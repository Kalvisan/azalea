use std::{
    collections::HashMap,
    io::{self, Cursor, Write},
};

use azalea_buf::{AzBuf, AzBufVar};
use azalea_chat::FormattedText;
use azalea_inventory::{DataComponentPatch, ItemStackData};
use azalea_protocol_macros::ClientboundGamePacket;
use azalea_registry::{builtin::ItemKind, identifier::Identifier};
use indexmap::IndexMap;

#[derive(AzBuf, ClientboundGamePacket, Clone, Debug, PartialEq)]
pub struct ClientboundUpdateAdvancements {
    pub should_reset: bool,
    pub added: Vec<PositionedAdvancement>,
    pub removed: Vec<Identifier>,
    pub progress: IndexMap<Identifier, AdvancementProgress>,
    pub show_advancements: bool,
}

#[derive(AzBuf, Clone, Debug, PartialEq)]
pub struct Advancement {
    pub parent_id: Option<Identifier>,
    pub display: Option<Box<DisplayInfo>>,
    pub requirements: Vec<Vec<String>>,
    pub sends_telemetry_event: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayInfo {
    pub title: FormattedText,
    pub description: FormattedText,
    /// The icon is an item stack template on the wire: the item, then the
    /// count as a varint, then the component patch. Unlike a slot, it can't be
    /// empty, so there's no leading count.
    pub icon: ItemStackData,
    pub frame: FrameType,
    pub show_toast: bool,
    pub hidden: bool,
    pub background: Option<Identifier>,
}

impl AzBuf for DisplayInfo {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, azalea_buf::BufReadError> {
        let title = AzBuf::azalea_read(buf)?;
        let description = AzBuf::azalea_read(buf)?;

        let kind = ItemKind::azalea_read(buf)?;
        let count = i32::azalea_read_var(buf)?;
        let component_patch = DataComponentPatch::azalea_read(buf)?;
        let icon = ItemStackData {
            kind,
            count,
            component_patch,
        };

        let frame = AzBuf::azalea_read(buf)?;

        let data = u32::azalea_read(buf)?;
        let has_background = (data & 0b1) != 0;
        let show_toast = (data & 0b10) != 0;
        let hidden = (data & 0b100) != 0;

        let background = if has_background {
            Some(Identifier::azalea_read(buf)?)
        } else {
            None
        };
        Ok(DisplayInfo {
            title,
            description,
            icon,
            frame,
            show_toast,
            hidden,
            background,
        })
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        self.title.azalea_write(buf)?;
        self.description.azalea_write(buf)?;

        self.icon.kind.azalea_write(buf)?;
        self.icon.count.azalea_write_var(buf)?;
        self.icon.component_patch.azalea_write(buf)?;

        self.frame.azalea_write(buf)?;

        let mut data: u32 = 0;
        if self.background.is_some() {
            data |= 0b001;
        }
        if self.show_toast {
            data |= 0b010;
        }
        if self.hidden {
            data |= 0b100;
        }
        data.azalea_write(buf)?;

        if let Some(background) = &self.background {
            background.azalea_write(buf)?;
        }
        Ok(())
    }
}

#[derive(AzBuf, Clone, Copy, Debug, PartialEq)]
pub enum FrameType {
    Task = 0,
    Challenge = 1,
    Goal = 2,
}

pub type AdvancementProgress = HashMap<String, CriterionProgress>;

#[derive(AzBuf, Clone, Debug, PartialEq)]
pub struct CriterionProgress {
    pub date: Option<u64>,
}

/// An advancement with its position in the advancements screen.
///
/// Since 26.3 the position is sent here instead of in [`DisplayInfo`].
#[derive(AzBuf, Clone, Debug, PartialEq)]
pub struct PositionedAdvancement {
    pub advancement: AdvancementHolder,
    pub x: f32,
    pub y: f32,
}

#[derive(AzBuf, Clone, Debug, PartialEq)]
pub struct AdvancementHolder {
    pub id: Identifier,
    pub value: Advancement,
}

#[cfg(test)]
mod tests {
    use azalea_buf::AzBuf;

    use super::*;

    #[test]
    fn test() {
        let packet = ClientboundUpdateAdvancements {
            should_reset: true,
            added: [PositionedAdvancement {
                advancement: AdvancementHolder {
                    id: Identifier::new("minecraft:test"),
                    value: Advancement {
                        parent_id: None,
                        display: Some(Box::new(DisplayInfo {
                            title: FormattedText::from("title".to_owned()),
                            description: FormattedText::from("description".to_owned()),
                            icon: ItemStackData::new(ItemKind::Stone, 1),
                            frame: FrameType::Task,
                            show_toast: true,
                            hidden: false,
                            background: None,
                        })),
                        requirements: Vec::new(),
                        sends_telemetry_event: false,
                    },
                },
                x: 1.23,
                y: 3.21,
            }]
            .into_iter()
            .collect(),
            removed: vec![Identifier::new("minecraft:test2")],
            progress: [(
                Identifier::new("minecraft:test3"),
                [(
                    "minecraft:test4".to_owned(),
                    CriterionProgress {
                        date: Some(123456789),
                    },
                )]
                .into_iter()
                .collect(),
            )]
            .into_iter()
            .collect(),
            show_advancements: false,
        };

        let mut data = Vec::new();
        packet.azalea_write(&mut data).unwrap();
        let mut buf: Cursor<&[u8]> = Cursor::new(&data);

        let read_packet = ClientboundUpdateAdvancements::azalea_read(&mut buf).unwrap();
        assert_eq!(packet.should_reset, read_packet.should_reset);
        assert_eq!(packet.removed, read_packet.removed);

        let advancement = packet
            .added
            .into_iter()
            .find_map(|a| {
                if a.advancement.id == Identifier::new("minecraft:test") {
                    Some(a.advancement.value)
                } else {
                    None
                }
            })
            .unwrap()
            .clone();
        let read_advancement = read_packet
            .added
            .into_iter()
            .find_map(|a| {
                if a.advancement.id == Identifier::new("minecraft:test") {
                    Some(a.advancement.value)
                } else {
                    None
                }
            })
            .unwrap()
            .clone();
        assert_eq!(advancement.parent_id, read_advancement.parent_id);

        let display = advancement.display.unwrap();
        let read_display = read_advancement.display.unwrap();
        assert_eq!(display.title, read_display.title);
        assert_eq!(display.description, read_display.description);
        assert_eq!(display.icon, read_display.icon);
    }
}
