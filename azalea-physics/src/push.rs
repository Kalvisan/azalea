//! Being pushed by the living entities the local player overlaps, as in
//! vanilla's `LivingEntity.pushEntities` and `Entity.push`.

use azalea_entity::{
    HasClientLoaded, LocalEntity, OnClimbable, Physics, Position,
    metadata::{AbstractLiving, ArmorStand, Bat},
};
use azalea_world::{WorldName, Worlds};
use bevy_ecs::prelude::*;

use crate::collision::entity_collisions::{AabbQuery, get_entities};

/// The share of the overlap direction an entity's push adds to the pushed
/// entity's motion (vanilla `Entity.push`, `0.05F`).
const PUSH_STRENGTH: f64 = 0.05f32 as f64;

/// Pushes each local player away from every pushable living entity whose
/// bounding box overlaps its own, after it has travelled this tick.
///
/// Vanilla runs `pushEntities` in `aiStep` after `travel`: for each such
/// entity, `entity.push(player)` moves the player by up to 0.05 along the
/// horizontal direction from the entity to the player, more for a deeper
/// overlap. A server's anticheat simulates the same push, so a client that
/// leaves it out drifts from its prediction while a mob stands next to it.
#[allow(clippy::type_complexity)]
pub fn push_entities(
    mut query: Query<
        (
            Entity,
            &mut Physics,
            &Position,
            &WorldName,
            Option<&OnClimbable>,
        ),
        (With<LocalEntity>, With<HasClientLoaded>),
    >,
    pushers: Query<(), (With<AbstractLiving>, Without<ArmorStand>, Without<Bat>)>,
    positions: Query<&Position>,
    aabb_query: AabbQuery,
    worlds: Res<Worlds>,
) {
    for (entity, mut physics, position, world_name, on_climbable) in &mut query {
        if on_climbable.is_some_and(|climbable| **climbable) {
            continue;
        }
        let Some(world_lock) = worlds.get(world_name) else {
            continue;
        };
        let world = world_lock.read();
        let pushable = |candidate| pushers.get(candidate).is_ok();
        let touching = get_entities(
            &world,
            Some(entity),
            &physics.bounding_box,
            &pushable,
            &aabb_query,
        );
        for (pusher, _) in touching {
            let Ok(pusher_position) = positions.get(pusher) else {
                continue;
            };
            let mut dx = position.x - pusher_position.x;
            let mut dz = position.z - pusher_position.z;
            let largest = dx.abs().max(dz.abs());
            if largest < 0.01f32 as f64 {
                continue;
            }
            let root = largest.sqrt();
            dx /= root;
            dz /= root;
            let scale = (1.0 / root).min(1.0);
            physics.velocity.x += dx * scale * PUSH_STRENGTH;
            physics.velocity.z += dz * scale * PUSH_STRENGTH;
        }
    }
}
