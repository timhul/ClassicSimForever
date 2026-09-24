//! Port of `Test/TestTalentTree`: spending points in one tab through the rules (tier
//! requirements, prerequisites, the point budget), as the talent UI does.

use crate::testing::warrior::WarriorTest;

pub(super) struct TalentTreeTest {
    pub test: WarriorTest,
    tab: &'static str,
    skill_line: u32,
}

impl TalentTreeTest {
    pub fn new(tab: &'static str) -> Self {
        let test = WarriorTest::new(tab);
        let skill_line = test
            .character()
            .talents()
            .and_then(|t| t.file().tabs.iter().find(|t| t.name == tab))
            .map(|t| t.skill_line)
            .unwrap_or_else(|| panic!("no talent tab {tab}"));
        TalentTreeTest {
            test,
            tab,
            skill_line,
        }
    }

    fn node(&self, name: &str) -> u32 {
        self.test
            .character()
            .talents()
            .and_then(|t| t.node_of_name(name, Some(self.skill_line)))
            .unwrap_or_else(|| panic!("no talent {name:?} in {}", self.tab))
    }

    /// Spends `times` points into talent `name`; whether every one of them went in.
    pub fn increment(&mut self, name: &str, times: u32) -> bool {
        let node = self.node(name);
        (0..times).fold(true, |all, _| {
            self.test.with_ctx(|ctx| ctx.increment_talent(node)) && all
        })
    }

    /// Takes `times` points out of talent `name`; whether every one of them came out.
    pub fn decrement(&mut self, name: &str, times: u32) -> bool {
        let node = self.node(name);
        (0..times).fold(true, |all, _| {
            self.test.with_ctx(|ctx| ctx.decrement_talent(node)) && all
        })
    }

    pub fn inc(&mut self, name: &str) -> bool {
        self.increment(name, 1)
    }

    pub fn dec(&mut self, name: &str) -> bool {
        self.decrement(name, 1)
    }

    pub fn tree_points(&self) -> u32 {
        self.test
            .character()
            .talents()
            .map_or(0, |t| t.tab_points(self.skill_line))
    }

    pub fn clear_tree(&mut self) {
        let skill_line = self.skill_line;
        self.test.with_ctx(|ctx| ctx.clear_talent_tab(skill_line));
        assert_eq!(self.tree_points(), 0);
    }

    pub fn switch_to_setup(&mut self, index: usize) {
        self.test.with_ctx(|ctx| ctx.switch_talent_setup(index));
    }
}
