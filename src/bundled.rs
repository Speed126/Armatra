//! Original MIT motion-only content. IDs are permanent; increment VERSION only
//! when adding a new ID. Completed IDs survive deletion of the library record.
pub const VERSION: u32 = 1;
pub struct Motion {
    pub id: &'static str,
    pub name: &'static str,
    pub bytes: &'static [u8],
}
macro_rules! motion {
    ($id:literal, $name:literal) => {
        Motion {
            id: $id,
            name: $name,
            bytes: include_bytes!(concat!("../assets/demo/motions/", $name, ".glb")),
        }
    };
}
pub const MOTIONS: &[Motion] = &[
    motion!("original-idle-v1", "Idle"),
    motion!("original-walk-v1", "Walk"),
    motion!("original-fast-walk-v1", "Fast-walk"),
    motion!("original-slow-walk-v1", "Slow-walk"),
    motion!("original-wave-v1", "Wave"),
    motion!("original-fast-wave-v1", "Fast-wave"),
    motion!("original-jump-v1", "Jump"),
    motion!("original-fast-jump-v1", "Fast-jump"),
    motion!("original-sway-v1", "Sway"),
];
