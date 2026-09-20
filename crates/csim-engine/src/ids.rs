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
    /// Index of a buff within the raid control's shared buff list (party buffs, shared debuffs).
    SharedBuffId(u32)
}

id_type! {
    /// Raid-wide unique identity of an enabled buff or spell (port of the C++ `InstanceID`). The
    /// target keys its debuff slots by it because buffs of every character (and the shared raid
    /// debuffs) compete for the same slots. Instead of the C++ central counter, the owner is
    /// encoded in the high byte: [`InstanceId::for_character`] for a character's own buffs and
    /// spells, [`InstanceId::for_raid`] for the buffs the raid control owns.
    InstanceId(u32)
}

impl InstanceId {
    /// Bits of the counter below the owner byte.
    const OWNER_SHIFT: u32 = 24;
    /// The owner byte reserved for the raid control's shared buffs (no character has it: the
    /// raid holds at most 40).
    const RAID_OWNER: u32 = 0xFF;

    /// The `counter`-th instance id handed out by `character`.
    pub fn for_character(character: CharId, counter: u32) -> Self {
        debug_assert!(u32::from(character.0) != Self::RAID_OWNER);
        debug_assert!(counter < 1 << Self::OWNER_SHIFT);
        InstanceId((u32::from(character.0) << Self::OWNER_SHIFT) | counter)
    }

    /// The `counter`-th instance id handed out by the raid control.
    pub fn for_raid(counter: u32) -> Self {
        debug_assert!(counter < 1 << Self::OWNER_SHIFT);
        InstanceId((Self::RAID_OWNER << Self::OWNER_SHIFT) | counter)
    }
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

    #[test]
    fn instance_ids_are_disjoint_between_owners() {
        assert_eq!(InstanceId::for_character(CharId(0), 5), InstanceId(5));
        assert_eq!(
            InstanceId::for_character(CharId(2), 1),
            InstanceId(0x0200_0001)
        );
        assert_eq!(InstanceId::for_raid(1), InstanceId(0xFF00_0001));
        assert_ne!(
            InstanceId::for_character(CharId(1), 0),
            InstanceId::for_raid(0)
        );
    }
}
