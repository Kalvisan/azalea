use azalea_core::{position::ChunkPos, tick::GameTick};
use azalea_entity::{
    HasClientLoaded, InLoadedChunk, LoadedBelow, LocalEntity, Position, update_in_loaded_chunk,
};
use azalea_physics::{PhysicsSystems, get_block_pos_below_that_affects_movement};
use azalea_protocol::packets::game::ServerboundPlayerLoaded;
use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;

use crate::{mining::MiningSystems, packet::game::SendGamePacketEvent};

pub struct PlayerLoadedPlugin;
impl Plugin for PlayerLoadedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            GameTick,
            // vanilla runs this on gameMode.tick()
            player_loaded_packet
                .after(update_in_loaded_chunk)
                .before(PhysicsSystems)
                .before(MiningSystems)
                .before(crate::movement::send_position),
        );
    }
}

// this component is removed on respawn or disconnect
// (notably, it's not removed on login)

// mojmap interchangeably calls it 'has client loaded' and 'has player loaded',
// i prefer the client one because it makes it clear that the component is only
// present on our own clients

#[allow(clippy::type_complexity)]
pub fn player_loaded_packet(
    mut commands: Commands,
    query: Query<
        Entity,
        (
            With<LocalEntity>,
            Without<HasClientLoaded>,
            // the vanilla client waits for the chunk mesh to be "compiled" for the renderer (or
            // some other conditions) before sending PlayerLoaded. see LevelLoadStatusManager.tick
            // in the decompiled source
            With<InLoadedChunk>,
        ),
    >,
) {
    for entity in query.iter() {
        commands.trigger(SendGamePacketEvent::new(entity, ServerboundPlayerLoaded));
        commands.entity(entity).insert(HasClientLoaded);
    }
}

/// Keeps each local player's [`LoadedBelow`] from its own chunk cache. Not
/// part of the pathfinder's simulation, whose players have no chunk cache of
/// their own and fall through the shared world.
pub struct LoadedBelowPlugin;
impl Plugin for LoadedBelowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(GameTick, update_loaded_below.before(PhysicsSystems));
    }
}

/// Records for each local player whether its own chunk cache has the chunk of
/// the block below it that affects its movement, as vanilla's
/// `level.hasChunkAt` asks the client's chunk cache.
pub fn update_loaded_below(
    mut commands: Commands,
    query: Query<(Entity, &Position, &crate::local_player::WorldHolder), With<LocalEntity>>,
) {
    for (entity, position, holder) in &query {
        let below = ChunkPos::from(get_block_pos_below_that_affects_movement(*position));
        let partial = holder.partial.read();
        let loaded =
            partial.chunks.in_range(&below) && partial.chunks.limited_get(&below).is_some();
        commands.entity(entity).insert(LoadedBelow(loaded));
    }
}
