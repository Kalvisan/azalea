use std::sync::LazyLock;

use azalea_block::{
    BlockState,
    fluid_state::{FluidKind, FluidState},
};
use azalea_core::{
    direction::Direction,
    position::{BlockPos, ChunkPos, Vec3},
};
use azalea_entity::{
    HasClientLoaded, LocalEntity, Physics, PlayerAbilities, Position,
    dimensions::EntityDimensions,
    metadata::{Sprinting, Swimming},
};
use azalea_registry::{builtin::BlockKind, tags};
use azalea_world::{World, WorldName, Worlds};
use bevy_ecs::prelude::*;

use crate::collision::{BlockWithShape, Shapes, VoxelShape, box_shape};

/// Updates which fluids the entity touches, how deep it is in them, whether its
/// eyes are in water, and pushes it with their currents, like vanilla's
/// `Entity.updateFluidInteraction` (with `EntityFluidInteraction.update`).
#[allow(clippy::type_complexity)]
pub fn update_in_water_state_and_do_fluid_pushing(
    mut query: Query<
        (&mut Physics, &Position, &EntityDimensions, &WorldName),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    worlds: Res<Worlds>,
) {
    for (mut physics, position, dimensions, world_name) in &mut query {
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        let world = world_lock.read();

        // Player.updateIsUnderwater and Entity.baseTick read the eye state
        // from the previous update
        physics.was_eye_in_water = physics.eye_in_water;

        let eye = Vec3::new(
            position.x,
            position.y + dimensions.eye_height as f64,
            position.z,
        );
        let interaction = FluidInteraction::update(&physics, &world, eye);

        physics.water_fluid_height = interaction.water.height;
        physics.lava_fluid_height = interaction.lava.height;
        physics.eye_in_water = interaction.eye_in_water;

        // TODO: implement vehicles and boats
        let in_water = interaction.water.height > 0.;
        if in_water {
            // if !was_touching_water && !first_tick {
            //     do_water_splash_effect();
            // }

            physics.reset_fall_distance();
            physics.clear_fire();
        }
        physics.was_touching_water = in_water;

        // right now doing registries.dimension_type() clones the entire
        // registry which is very inefficient, so for now we're doing
        // this instead

        let is_ultrawarm = world
            .registries
            .dimension_type
            .map
            .get(&**world_name)
            .and_then(|i| i.ultrawarm)
            .unwrap_or_default();
        let lava_push_factor = if is_ultrawarm {
            0.007
        } else {
            0.0023333333333333335
        };

        if in_water {
            interaction.water.apply_to(&mut physics, 0.014);
        }
        if interaction.lava.height > 0. {
            interaction.lava.apply_to(&mut physics, lava_push_factor);
        }
    }
}

/// What one fluid kind does to the entity this tick: how far it reaches above
/// the entity's feet, and the summed flow of the touched blocks.
#[derive(Default)]
struct FluidContact {
    height: f64,
    current_height: f64,
    current: Vec3,
    current_count: u32,
}

impl FluidContact {
    /// `EntityFluidInteraction.CurrentAccumulator.applyTo` for a player.
    fn apply_to(&self, physics: &mut Physics, scale: f64) {
        if self.current_count == 0 || self.current.length_squared() < 1.0e-5f32 as f64 {
            return;
        }
        let mut impulse = self.current * (1. / self.current_count as f64) * scale;
        const MIN_PUSH: f64 = 0.003;
        const MIN_PUSH_LENGTH: f64 = 0.0045000000000000005;
        if physics.velocity.x.abs() < MIN_PUSH
            && physics.velocity.z.abs() < MIN_PUSH
            && impulse.length() < MIN_PUSH_LENGTH
        {
            impulse = impulse.normalize() * MIN_PUSH_LENGTH;
        }
        physics.velocity += impulse;
    }
}

/// The fluids around the entity this tick, as `EntityFluidInteraction`
/// collects them.
#[derive(Default)]
struct FluidInteraction {
    water: FluidContact,
    lava: FluidContact,
    eye_in_water: bool,
}

impl FluidInteraction {
    fn update(physics: &Physics, world: &World, eye: Vec3) -> Self {
        let mut interaction = Self::default();
        // Entity.getFluidInteractionBox
        let bounding_box = physics.bounding_box.deflate_all(0.001);
        let feet_y = physics.bounding_box.min.y;

        let min_x = bounding_box.min.x.floor() as i32;
        let min_y = bounding_box.min.y.floor() as i32;
        let min_z = bounding_box.min.z.floor() as i32;
        let max_x = bounding_box.max.x.ceil() as i32;
        let max_y = bounding_box.max.y.ceil() as i32;
        let max_z = bounding_box.max.z.ceil() as i32;

        let eye_block_x = eye.x.floor() as i32;
        let eye_block_z = eye.z.floor() as i32;

        for x in min_x..max_x {
            for y in min_y..max_y {
                for z in min_z..max_z {
                    let pos = BlockPos::new(x, y, z);
                    let Some(fluid) = world.get_fluid_state(pos) else {
                        // EntityFluidInteraction.hasFluidAndLoaded: an
                        // unloaded chunk means no fluid at all, while a
                        // block outside the world's height is just empty
                        if world.chunks.get(&ChunkPos::from(pos)).is_none() {
                            return Self::default();
                        }
                        continue;
                    };
                    if fluid.is_empty() {
                        continue;
                    }
                    let fluid_bottom = y as f64;
                    let fluid_top = fluid_bottom + fluid_height(&fluid, world, pos) as f64;
                    if fluid_top < bounding_box.min.y {
                        continue;
                    }
                    if fluid.kind == FluidKind::Water
                        && x == eye_block_x
                        && z == eye_block_z
                        && eye.y >= fluid_bottom
                        && eye.y
                            <= fluid_bottom + fluid_height_for_camera(&fluid, world, pos) as f64
                    {
                        interaction.eye_in_water = true;
                    }
                    let contact = match fluid.kind {
                        FluidKind::Water => &mut interaction.water,
                        FluidKind::Lava => &mut interaction.lava,
                        FluidKind::Empty => continue,
                    };
                    contact.height = f64::max(fluid_top - feet_y, contact.height);
                    let mut flow = get_fluid_flow(&fluid, world, pos);
                    contact.current_height = f64::max(contact.height, contact.current_height);
                    if contact.current_height < 0.4 {
                        flow *= contact.current_height;
                    }
                    contact.current += flow;
                    contact.current_count += 1;
                }
            }
        }
        interaction
    }
}

/// `FluidState.getHeight`: a full block when the same fluid is above,
/// otherwise the fluid's own height.
pub fn fluid_height(fluid: &FluidState, world: &World, pos: BlockPos) -> f32 {
    let above = world.get_fluid_state(pos.up(1)).unwrap_or_default();
    if !fluid.is_empty() && above.kind == fluid.kind {
        1.
    } else {
        fluid.height()
    }
}

/// `FluidState.getHeightForCamera`: a source block under a block with a sturdy
/// bottom face counts as full.
fn fluid_height_for_camera(fluid: &FluidState, world: &World, pos: BlockPos) -> f32 {
    if fluid.amount == 8 {
        let above_pos = pos.up(1);
        let above = world.get_block_state(above_pos).unwrap_or_default();
        if is_face_sturdy(above, above_pos, Direction::Down) {
            return 1.;
        }
    }
    fluid_height(fluid, world, pos)
}

/// `BlockState.isFaceSturdy` with the full support type, from the collision
/// shape: the block fills the whole square of its cell on the `direction`
/// side. Vanilla reads the support shape, which differs from the collision
/// shape for few blocks.
fn is_face_sturdy(state: BlockState, pos: BlockPos, direction: Direction) -> bool {
    static FACES: LazyLock<[VoxelShape; 6]> = LazyLock::new(|| {
        const T: f64 = 1. / 16.;
        [
            box_shape(0., 0., 0., 1., T, 1.),
            box_shape(0., 1. - T, 0., 1., 1., 1.),
            box_shape(0., 0., 0., 1., 1., T),
            box_shape(0., 0., 1. - T, 1., 1., 1.),
            box_shape(0., 0., 0., T, 1., 1.),
            box_shape(1. - T, 0., 0., 1., 1., 1.),
        ]
    });
    let face = &FACES[direction as usize];
    let shape = state.collision_shape(pos);
    !Shapes::matches_anywhere(face, &shape, |face, block| face && !block)
}

/// `Player.updateSwimming`: sprinting under water starts swimming, which
/// lasts while the entity keeps sprinting in water and is not flying.
#[allow(clippy::type_complexity)]
pub fn update_swimming(
    mut query: Query<
        (
            &mut Swimming,
            &Sprinting,
            &Physics,
            &Position,
            &WorldName,
            Option<&PlayerAbilities>,
        ),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    worlds: Res<Worlds>,
) {
    for (mut swimming, sprinting, physics, position, world_name, abilities) in &mut query {
        // TODO: passengers never swim
        let swims = if abilities.is_some_and(|abilities| abilities.flying) {
            false
        } else if **swimming {
            **sprinting && physics.is_in_water()
        } else {
            let Some(world_lock) = worlds.get(world_name) else {
                continue;
            };
            let in_water_block = world_lock
                .read()
                .get_fluid_state(BlockPos::from(*position))
                .is_some_and(|fluid| fluid.kind == FluidKind::Water);
            **sprinting && physics.is_under_water() && in_water_block
        };
        if **swimming != swims {
            **swimming = swims;
        }
    }
}

// FlowingFluid.getFlow
pub fn get_fluid_flow(fluid: &FluidState, world: &World, pos: BlockPos) -> Vec3 {
    let mut z_flow: f64 = 0.;
    let mut x_flow: f64 = 0.;

    let cur_fluid_height = fluid.height();

    for direction in Direction::HORIZONTAL {
        let adjacent_block_pos = pos.offset_with_direction(direction);

        let adjacent_block_state = world
            .get_block_state(adjacent_block_pos)
            .unwrap_or_default();
        let adjacent_fluid_state = FluidState::from(adjacent_block_state);

        if !fluid.affects_flow(&adjacent_fluid_state) {
            continue;
        };
        let mut adjacent_fluid_height = adjacent_fluid_state.height();
        let mut adjacent_height_difference: f32 = 0.;

        if adjacent_fluid_height == 0. {
            if !tags::blocks::BLOCKS_FLUID_FLOW.contains(&BlockKind::from(adjacent_block_state)) {
                let block_pos_below_adjacent = adjacent_block_pos.down(1);
                let fluid_below_adjacent = world
                    .get_fluid_state(block_pos_below_adjacent)
                    .unwrap_or_default();

                if fluid.affects_flow(&fluid_below_adjacent) {
                    adjacent_fluid_height = fluid_below_adjacent.height();
                    if adjacent_fluid_height > 0. {
                        adjacent_height_difference =
                            cur_fluid_height - (adjacent_fluid_height - 0.8888889);
                    }
                }
            }
        } else if adjacent_fluid_height > 0. {
            adjacent_height_difference = cur_fluid_height - adjacent_fluid_height;
        }

        if adjacent_height_difference != 0. {
            x_flow += (direction.x() as f32 * adjacent_height_difference) as f64;
            z_flow += (direction.z() as f32 * adjacent_height_difference) as f64;
        }
    }

    let mut flow = Vec3::new(x_flow, 0., z_flow);
    if fluid.falling {
        for direction in Direction::HORIZONTAL {
            let adjacent_block_pos = pos.offset_with_direction(direction);
            if is_solid_face(fluid, world, adjacent_block_pos, direction)
                || is_solid_face(fluid, world, adjacent_block_pos.up(1), direction)
            {
                flow = flow.normalize() + Vec3::new(0., -6., 0.);
                break;
            }
        }
    }

    flow.normalize()
}

/// `FlowingFluid.isSolidFace`: whether the block beside falling fluid turns
/// its flow downwards.
fn is_solid_face(
    fluid: &FluidState,
    world: &World,
    adjacent_pos: BlockPos,
    direction: Direction,
) -> bool {
    let block_state = world.get_block_state(adjacent_pos).unwrap_or_default();
    let fluid_state = world.get_fluid_state(adjacent_pos).unwrap_or_default();
    if fluid_state.is_same_kind(fluid) {
        return false;
    }
    if direction == Direction::Up {
        return true;
    }
    if matches!(
        BlockKind::from(block_state),
        BlockKind::Ice | BlockKind::FrostedIce
    ) {
        return false;
    }
    is_face_sturdy(block_state, adjacent_pos, direction)
}
