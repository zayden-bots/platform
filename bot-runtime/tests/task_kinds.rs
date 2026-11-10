use async_trait::async_trait;
use bot_runtime::{BotError, ModuleList, RegistryBuilder};
use zayden_core::{Module, OverlapError};

struct Dummy {
    id: &'static str,
    kinds: &'static [&'static str],
}

#[async_trait]
impl Module for Dummy {
    fn id(&self) -> &'static str {
        self.id
    }

    fn register(&self, _b: &mut RegistryBuilder) -> Result<(), OverlapError> {
        Ok(())
    }

    fn task_kinds(&self) -> &'static [&'static str] {
        self.kinds
    }
}

#[test]
fn duplicate_task_kinds_fail_finish() {
    let mut list = ModuleList::default();
    list.add(Dummy { id: "a", kinds: &["dup"] });
    list.add(Dummy { id: "b", kinds: &["dup"] });

    assert!(matches!(list.finish(vec![]), Err(BotError::DuplicateTaskKind("dup"))));
}

#[test]
fn distinct_task_kinds_are_accepted() {
    let mut list = ModuleList::default();
    list.add(Dummy { id: "a", kinds: &["one"] });
    list.add(Dummy { id: "b", kinds: &["two"] });

    let Ok(assembly) = list.finish(vec![]) else {
        panic!("distinct kinds must assemble");
    };
    let kinds = assembly
        .modules
        .iter()
        .flat_map(|module| module.task_kinds())
        .copied()
        .collect::<Vec<_>>();

    assert_eq!(kinds, ["one", "two"]);
}
