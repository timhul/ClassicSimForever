//! Typed handles used instead of the raw back-pointers of the C++ engine.
//!
//! A handle is an index into the owning collection (e.g. `SpellId` indexes the spell vector of the
//! character it belongs to). Handles are plain `Copy` values so events and specs can refer to
//! simulation objects without borrowing them.

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident($inner:ty)) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub $inner);

        impl $name {
            /// Returns the handle as a collection index.
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }

        impl From<$name> for usize {
            fn from(id: $name) -> usize {
                id.index()
            }
        }
    };
}

id_type! {
    /// Index of a character within the raid.
    CharId(u8)
}

id_type! {
    /// Index of a spell within its character's spell list.
    SpellId(u32)
}

id_type! {
    /// Index of a buff within its character's buff list (or the shared raid/party buff lists).
    BuffId(u32)
}

id_type! {
    /// Index of a proc within its character's proc list.
    ProcId(u32)
}

id_type! {
    /// Index of a cooldown control within its character's cooldown registry.
    CooldownId(u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_convert_to_indices() {
        assert_eq!(CharId(3).index(), 3);
        assert_eq!(usize::from(SpellId(7)), 7);
        assert_eq!(BuffId(1), BuffId(1));
        assert!(ProcId(1) < ProcId(2));
    }
}
