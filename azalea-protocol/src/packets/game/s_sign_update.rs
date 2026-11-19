use azalea_buf::AzBuf;
use azalea_core::position::BlockPos;
use azalea_protocol_macros::ServerboundGamePacket;

#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundGamePacket)]
pub struct ServerboundSignUpdate {
    pub pos: BlockPos,
    pub lines: [String; 4],
    /// The side written, sent after the lines as 26.3's sign text slot
    /// (a varint: 0 the back, 1 the front, the same bytes as this bool).
    pub is_front_text: bool,
}
