//! The client's side of vanilla's `MultiPlayerGameMode.performUseItemOn`: whether a right click on a block takes the
//! click, so that `Minecraft.startUseItem` sends no `ServerboundUseItem` after the `ServerboundUseItemOn`.
//!
//! Vanilla runs the block's and the item's use code on the client to know this. Azalea has no such code, so this module
//! mirrors the client-side results of the 26.3 blocks and items that take a click, read from their
//! `useItemOn`/`useWithoutItem`/`useOn` methods. A few depend on what the client cannot see (a block entity's contents,
//! where on the block the click landed); those are counted as taking the click, which only leaves out the item use that
//! vanilla would have sent.

use azalea_block::{BlockState, properties};
use azalea_core::{direction::Direction, game_type::GameMode, hit_result::BlockHitResult};
use azalea_inventory::{ItemStack, components};
use azalea_protocol::packets::game::s_interact::InteractionHand;
use azalea_registry::{
    builtin::{BlockKind, ItemKind, Potion},
    tags,
};

use crate::local_player::RecipePropertySets;

/// What the player brings to a right click on a block, beside the block itself.
pub struct UseOnContext<'a> {
    pub hand: InteractionHand,
    /// The item in `hand`.
    pub item: &'a ItemStack,
    /// The item in the other hand.
    pub other_item: &'a ItemStack,
    /// The shift key, `Player.isSecondaryUseActive`.
    pub secondary: bool,
    pub game_mode: GameMode,
    /// `Player.canUseGameMasterBlocks`: creative and an operator.
    pub game_master: bool,
    /// The food level, which decides whether a cake can be eaten.
    pub food: u32,
    pub recipe_sets: Option<&'a RecipePropertySets>,
}

/// Whether vanilla's client would send `ServerboundUseItem` right after the `ServerboundUseItemOn` for this click: the
/// held item is not empty, and neither the block nor the item's `useOn` took the click or failed.
pub fn item_use_follows(block: BlockState, hit: &BlockHitResult, ctx: &UseOnContext) -> bool {
    if ctx.item.is_empty() || ctx.game_mode == GameMode::Spectator {
        return false;
    }
    // with the shift key down, a player holding anything skips the block's own use
    let suppress_block = ctx.secondary && (ctx.item.is_present() || ctx.other_item.is_present());
    if !suppress_block && block_takes_click(block, ctx) {
        return false;
    }
    !item_use_on_stops(block, hit, ctx.item)
}

/// Whether the block's `useItemOn`, or its `useWithoutItem` when that answers `TRY_WITH_EMPTY_HAND` in the main hand,
/// consumes the click on the client.
fn block_takes_click(block: BlockState, ctx: &UseOnContext) -> bool {
    let kind = BlockKind::from(block);
    let item = ctx.item.kind();
    let may_build = ctx.game_mode != GameMode::Adventure;
    let main_hand = ctx.hand == InteractionHand::MainHand;
    let with_item = match kind {
        _ if tags::blocks::CAULDRONS.contains(&kind) => Use::takes(cauldron_takes(kind, block, ctx.item)),
        _ if tags::blocks::CAMPFIRES.contains(&kind) => {
            let input = ctx.recipe_sets.is_some_and(|sets| sets.contains("minecraft:campfire_input", item));
            let douses = tags::items::DOUSES_CAMPFIRES.contains(&item)
                && block.property::<properties::Lit>().unwrap_or(false);
            Use::takes(input || douses)
        }
        _ if tags::blocks::COPPER_GOLEM_STATUES.contains(&kind) => {
            // the weathering statues pass honeycomb on to be waxed; the waxed ones change pose
            let weathering = !kind.to_str().starts_with("minecraft:waxed_");
            Use::Done(!tags::items::AXES.contains(&item) && !(weathering && item == ItemKind::Honeycomb))
        }
        _ if tags::blocks::CANDLE_CAKES.contains(&kind) => {
            if matches!(item, ItemKind::FlintAndSteel | ItemKind::FireCharge) {
                Use::Done(false)
            } else {
                Use::EmptyHand
            }
        }
        _ if tags::blocks::FLOWER_POTS.contains(&kind) => Use::EmptyHand,
        _ if tags::blocks::REDSTONE_ORES.contains(&kind) => Use::Done(true),
        _ if tags::blocks::ALL_SIGNS.contains(&kind) => Use::Done(true),
        _ if tags::blocks::BEEHIVES.contains(&kind) => {
            let full = block.property::<properties::HoneyLevel>().is_some_and(|level| level as u8 >= 5);
            Use::takes(full && item == ItemKind::GlassBottle)
        }
        BlockKind::Cake => Use::EmptyHand,
        BlockKind::Composter => {
            let level = block.property::<properties::ComposterLevel>().map_or(0, |level| level as u8);
            let compostable = ctx.item.get_component::<components::Compostable>().is_some();
            Use::takes(level < 8 && compostable)
        }
        BlockKind::Jukebox => {
            let playable = ctx.item.get_component::<components::JukeboxPlayable>().is_some();
            let has_record = block.property::<properties::HasRecord>().unwrap_or(false);
            Use::takes(!has_record && playable)
        }
        BlockKind::Lectern => {
            let has_book = block.property::<properties::HasBook>().unwrap_or(false);
            Use::takes(!has_book && tags::items::LECTERN_BOOKS.contains(&item))
        }
        BlockKind::RespawnAnchor => {
            let chargeable = block.property::<properties::Charges>().is_some_and(|charges| (charges as u8) < 4);
            if item == ItemKind::Glowstone && chargeable {
                Use::Done(true)
            } else if main_hand && ctx.other_item.kind() == ItemKind::Glowstone && chargeable {
                // leaves the charging to the off hand
                Use::Done(false)
            } else {
                Use::EmptyHand
            }
        }
        BlockKind::DecoratedPot => Use::Done(true),
        BlockKind::Pumpkin => Use::takes(item == ItemKind::Shears),
        BlockKind::Tnt => Use::takes(matches!(item, ItemKind::FlintAndSteel | ItemKind::FireCharge)),
        BlockKind::SweetBerryBush => {
            let ripe = block.property::<properties::SweetBerryBushAge>().is_some_and(|age| age as u8 == 3);
            if item == ItemKind::BoneMeal && !ripe {
                Use::Done(false)
            } else {
                Use::EmptyHand
            }
        }
        BlockKind::Vault => {
            let active = block.property::<properties::VaultState>() == Some(properties::VaultState::Active);
            Use::takes(active)
        }
        BlockKind::ChiseledBookshelf => Use::EmptyHand,
        _ if is_shelf(kind) => Use::Done(main_hand),
        _ => Use::EmptyHand,
    };
    match with_item {
        Use::Done(taken) => taken,
        Use::EmptyHand => main_hand && use_without_item_takes(kind, block, ctx, may_build),
    }
}

/// A block's answer to the click with the item, before `useWithoutItem`.
enum Use {
    /// The click is decided: taken or passed on to the item.
    Done(bool),
    /// `TRY_WITH_EMPTY_HAND`: the main hand goes on to `useWithoutItem`.
    EmptyHand,
}

impl Use {
    /// Taken when `taken`, else on to `useWithoutItem`.
    fn takes(taken: bool) -> Self {
        if taken { Use::Done(true) } else { Use::EmptyHand }
    }
}

/// Whether the block's `useWithoutItem` consumes the click on the client.
fn use_without_item_takes(
    kind: BlockKind,
    block: BlockState,
    ctx: &UseOnContext,
    may_build: bool,
) -> bool {
    match kind {
        _ if tags::blocks::BEDS.contains(&kind)
            || tags::blocks::BUTTONS.contains(&kind)
            || tags::blocks::FENCE_GATES.contains(&kind)
            || tags::blocks::SHULKER_BOXES.contains(&kind)
            || tags::blocks::ANVIL.contains(&kind)
            || tags::blocks::COPPER_CHESTS.contains(&kind)
            || tags::blocks::FLOWER_POTS.contains(&kind) =>
        {
            true
        }
        _ if tags::blocks::DOORS.contains(&kind) => kind != BlockKind::IronDoor,
        _ if tags::blocks::TRAPDOORS.contains(&kind) => kind != BlockKind::IronTrapdoor,
        _ if tags::blocks::CAVE_VINES.contains(&kind) => {
            block.property::<properties::Berries>().unwrap_or(false)
        }
        _ if tags::blocks::CANDLE_CAKES.contains(&kind) => can_eat(ctx),
        BlockKind::Cake => can_eat(ctx),
        BlockKind::Furnace
        | BlockKind::BlastFurnace
        | BlockKind::Smoker
        | BlockKind::Barrel
        | BlockKind::Beacon
        | BlockKind::BrewingStand
        | BlockKind::Chest
        | BlockKind::TrappedChest
        | BlockKind::Crafter
        | BlockKind::CraftingTable
        | BlockKind::Dispenser
        | BlockKind::Dropper
        | BlockKind::DragonEgg
        | BlockKind::EnchantingTable
        | BlockKind::EnderChest
        | BlockKind::Grindstone
        | BlockKind::Hopper
        | BlockKind::Lever
        | BlockKind::Loom
        | BlockKind::NoteBlock
        | BlockKind::CartographyTable
        | BlockKind::SmithingTable
        | BlockKind::Stonecutter
        | BlockKind::Light
        | BlockKind::Lectern
        | BlockKind::Bell
        | BlockKind::ChiseledBookshelf => true,
        BlockKind::Comparator | BlockKind::Repeater | BlockKind::DaylightDetector => may_build,
        // only a dot or a cross toggles; a wire line passes
        BlockKind::RedstoneWire => may_build && wire_toggles(block),
        BlockKind::CommandBlock
        | BlockKind::ChainCommandBlock
        | BlockKind::RepeatingCommandBlock
        | BlockKind::Jigsaw
        | BlockKind::StructureBlock
        | BlockKind::TestBlock
        | BlockKind::TestInstanceBlock => ctx.game_master,
        BlockKind::Composter => {
            block.property::<properties::ComposterLevel>().is_some_and(|level| level as u8 == 8)
        }
        BlockKind::Jukebox => block.property::<properties::HasRecord>().unwrap_or(false),
        BlockKind::RespawnAnchor => block.property::<properties::Charges>().is_some_and(|charges| charges as u8 > 0),
        BlockKind::SweetBerryBush => {
            block.property::<properties::SweetBerryBushAge>().is_some_and(|age| age as u8 > 1)
        }
        _ => false,
    }
}

/// `Player.canEat(false)`: hungry, or in creative.
fn can_eat(ctx: &UseOnContext) -> bool {
    ctx.food < 20 || ctx.game_mode == GameMode::Creative
}

/// `RedStoneWireBlock.isCross || isDot`: connected on all four sides, or on none.
fn wire_toggles(block: BlockState) -> bool {
    use properties::{WireEast, WireNorth, WireSouth, WireWest};
    let none = [
        block.property::<WireNorth>() == Some(WireNorth::None),
        block.property::<WireEast>() == Some(WireEast::None),
        block.property::<WireSouth>() == Some(WireSouth::None),
        block.property::<WireWest>() == Some(WireWest::None),
    ];
    none.iter().all(|side| *side) || none.iter().all(|side| !side)
}

/// The shelves, whose `useItemOn` takes the main hand's click on the client whenever the held item is not empty.
fn is_shelf(kind: BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::AcaciaShelf
            | BlockKind::BambooShelf
            | BlockKind::BirchShelf
            | BlockKind::CherryShelf
            | BlockKind::CrimsonShelf
            | BlockKind::DarkOakShelf
            | BlockKind::JungleShelf
            | BlockKind::MangroveShelf
            | BlockKind::OakShelf
            | BlockKind::PaleOakShelf
            | BlockKind::PoplarShelf
            | BlockKind::SpruceShelf
            | BlockKind::WarpedShelf
    )
}

/// Whether a cauldron has an interaction for `item` in vanilla's `CauldronInteractions`: emptying a bucket, or a water
/// bottle into a cauldron that is not full; filling a bucket from a full cauldron or a bottle from water; and washing
/// the dye off a dyed item in water. Banners and shulker boxes are block items, which end the click anyway.
fn cauldron_takes(kind: BlockKind, block: BlockState, item: &ItemStack) -> bool {
    let water_level = block.property::<properties::WaterCauldronLevel>().map(|level| level as u8 + 1);
    let snow_level = block.property::<properties::PowderSnowCauldronLevel>().map(|level| level as u8 + 1);
    let full = match kind {
        BlockKind::WaterCauldron => water_level == Some(3),
        BlockKind::PowderSnowCauldron => snow_level == Some(3),
        BlockKind::LavaCauldron => true,
        _ => false,
    };
    match item.kind() {
        ItemKind::WaterBucket | ItemKind::LavaBucket | ItemKind::PowderSnowBucket => true,
        ItemKind::Potion => {
            is_water_bottle(item)
                && (kind == BlockKind::Cauldron || (kind == BlockKind::WaterCauldron && water_level < Some(3)))
        }
        ItemKind::Bucket => full,
        ItemKind::GlassBottle => kind == BlockKind::WaterCauldron,
        kind_of_item => {
            kind == BlockKind::WaterCauldron
                && tags::items::CAULDRON_CAN_REMOVE_DYE.contains(&kind_of_item)
                && item.get_component::<components::DyedColor>().is_some()
        }
    }
}

fn is_water_bottle(item: &ItemStack) -> bool {
    item.get_component::<components::PotionContents>()
        .is_some_and(|contents| contents.potion == Some(Potion::Water))
}

/// Whether the item's `useOn` ends the click on the client. Exact for block items, which place or fail; for the items
/// whose `useOn` always answers on the client (fire starters, spawn eggs, entity placers and cushions, the brush and debug stick);
/// for a compass on a lodestone, shears on a growing plant's head, a water bottle turning dirt to mud, and for an eye of ender filling an end portal frame. Items whose `useOn`
/// works only on some blocks and that have no `use` of their own (tools, bone meal, honeycomb)
/// are counted as ending it, as the module notes; their `use` would do nothing anyway.
fn item_use_on_stops(block: BlockState, hit: &BlockHitResult, item: &ItemStack) -> bool {
    let kind = item.kind();
    let transforms_blocks = [&tags::items::AXES, &tags::items::HOES, &tags::items::SHOVELS]
        .iter()
        .any(|tag| tag.contains(&kind));
    let name = kind.to_str();
    if is_block_item(kind) || transforms_blocks || name.ends_with("_spawn_egg") || name.ends_with("_cushion") {
        return true;
    }
    match kind {
        ItemKind::Potion => {
            is_water_bottle(item)
                && hit.direction != Direction::Down
                && tags::blocks::CONVERTIBLE_TO_MUD.contains(&BlockKind::from(block))
        }
        ItemKind::EnderEye => {
            BlockKind::from(block) == BlockKind::EndPortalFrame
                && !block.property::<properties::Eye>().unwrap_or(false)
        }
        ItemKind::Compass => BlockKind::from(block) == BlockKind::Lodestone,
        ItemKind::Shears => matches!(
            BlockKind::from(block),
            BlockKind::Kelp | BlockKind::WeepingVines | BlockKind::TwistingVines | BlockKind::CaveVines
        ),
        _ => matches!(
            kind,
            ItemKind::FireworkRocket
                | ItemKind::FlintAndSteel
                | ItemKind::FireCharge
                | ItemKind::Minecart
                | ItemKind::ChestMinecart
                | ItemKind::FurnaceMinecart
                | ItemKind::HopperMinecart
                | ItemKind::TntMinecart
                | ItemKind::CommandBlockMinecart
                | ItemKind::ArmorStand
                | ItemKind::EndCrystal
                | ItemKind::Painting
                | ItemKind::ItemFrame
                | ItemKind::GlowItemFrame
                | ItemKind::Brush
                | ItemKind::DebugStick
                | ItemKind::BoneMeal
                | ItemKind::Honeycomb
        ),
    }
}

/// Whether vanilla registers `item` as a `BlockItem`: the items named after their block, and the few that
/// `Items.createBlockItemWithCustomItemName` names otherwise. Lily pads and frogspawn are left out: their `useOn` passes
/// and their `use` places them on water.
fn is_block_item(item: ItemKind) -> bool {
    if matches!(item, ItemKind::LilyPad | ItemKind::Frogspawn) {
        return false;
    }
    matches!(
        item,
        ItemKind::Redstone
            | ItemKind::String
            | ItemKind::WheatSeeds
            | ItemKind::PowderSnowBucket
            | ItemKind::CocoaBeans
            | ItemKind::PumpkinSeeds
            | ItemKind::MelonSeeds
            | ItemKind::Carrot
            | ItemKind::Potato
            | ItemKind::TorchflowerSeeds
            | ItemKind::PitcherPod
            | ItemKind::BeetrootSeeds
            | ItemKind::SweetBerries
            | ItemKind::GlowBerries
    ) || item.to_str().parse::<BlockKind>().is_ok()
}

#[cfg(test)]
mod tests {
    use azalea_core::position::{BlockPos, Vec3};

    use super::*;

    /// Whether a main-hand click with `item` on `block` goes on to useItem.
    fn follows(block: impl Into<BlockState>, item: ItemKind, food: u32) -> bool {
        let item = ItemStack::from(item);
        let hit = BlockHitResult {
            location: Vec3::ZERO,
            miss: false,
            direction: Direction::Up,
            block_pos: BlockPos::new(0, 0, 0),
            inside: false,
            world_border: false,
        };
        let ctx = UseOnContext {
            hand: InteractionHand::MainHand,
            item: &item,
            other_item: &ItemStack::Empty,
            secondary: false,
            game_mode: GameMode::Survival,
            game_master: false,
            food,
            recipe_sets: None,
        };
        item_use_follows(block.into(), &hit, &ctx)
    }

    #[test]
    fn food_is_eaten_past_a_block_that_takes_nothing() {
        assert!(follows(BlockKind::GrassBlock, ItemKind::Bread, 10));
        assert!(follows(BlockKind::Stone, ItemKind::CookedBeef, 10));
    }

    #[test]
    fn a_block_with_a_use_takes_the_click() {
        assert!(!follows(BlockKind::Chest, ItemKind::Bread, 10));
        assert!(!follows(BlockKind::OakDoor, ItemKind::Bread, 10));
        assert!(!follows(BlockKind::CraftingTable, ItemKind::Bread, 10));
        assert!(follows(BlockKind::IronDoor, ItemKind::Bread, 10));
        // a cake is eaten only by a hungry player
        assert!(!follows(BlockKind::Cake, ItemKind::Bread, 10));
        assert!(follows(BlockKind::Cake, ItemKind::Bread, 20));
    }

    #[test]
    fn a_block_item_never_falls_back() {
        assert!(!follows(BlockKind::Stone, ItemKind::Cobblestone, 10));
        assert!(!follows(BlockKind::Stone, ItemKind::WheatSeeds, 10));
        // a lily pad places through use, so the fallback is how it is placed
        assert!(follows(BlockKind::Stone, ItemKind::LilyPad, 10));
    }

    #[test]
    fn an_item_whose_use_on_answers_takes_the_click() {
        assert!(!follows(BlockKind::Stone, ItemKind::FlintAndSteel, 10));
        assert!(!follows(BlockKind::GrassBlock, ItemKind::WoodenHoe, 10));
        assert!(!follows(BlockKind::Stone, ItemKind::PigSpawnEgg, 10));
        assert!(!follows(BlockKind::Stone, ItemKind::Painting, 10));
        // a stick has neither, so vanilla sends the item use anyway
        assert!(follows(BlockKind::Stone, ItemKind::Stick, 10));
        // a lead binds mobs only on the server, and a compass takes only a lodestone
        assert!(follows(BlockKind::OakFence, ItemKind::Lead, 10));
        assert!(follows(BlockKind::Stone, ItemKind::Compass, 10));
        assert!(!follows(BlockKind::Lodestone, ItemKind::Compass, 10));
    }

    #[test]
    fn a_cauldron_takes_only_what_it_can_hold_or_give() {
        // an empty bucket fills only from a full cauldron
        assert!(!follows(BlockKind::LavaCauldron, ItemKind::Bucket, 10));
        assert!(follows(BlockKind::Cauldron, ItemKind::Bucket, 10));
        // an undyed leather tunic is equipped, not washed
        assert!(follows(BlockKind::WaterCauldron, ItemKind::LeatherChestplate, 10));
    }
}
