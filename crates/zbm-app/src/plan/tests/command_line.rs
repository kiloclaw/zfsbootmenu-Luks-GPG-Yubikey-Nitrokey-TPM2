//! The plans of a command line saved or inherited: what they would run,
//! quoted as a shell takes it, and the lines they refuse before anything.

use pretty_assertions::assert_eq;
use zbm_types::{DatasetName, Untrusted};

use super::*;

/// The plan of `action` on `fake`, through the dry run a menu asks for.
fn planned(fake: &FakeSystem, action: &Action) -> Plan {
    let runtime = Runtime::default().with_base(Path::new("/zfsbootmenu"));
    let app = App::new(Context::new(fake.system()), &runtime);
    let events = app.apply(
        &Action::Plan(Box::new(action.clone())),
        Through::Quietly(fake.system()),
    );
    let [Event::Planned { plan, .. }] = events.as_slice() else {
        panic!("not a plan: {events:?}");
    };
    plan.clone()
}

fn environment() -> DatasetName {
    "tank/ROOT/be".parse().unwrap()
}

fn line(text: &str) -> Untrusted<String> {
    Untrusted::new(text.to_owned())
}

#[test]
fn a_read_only_pool_is_imported_read_write_first() {
    let fake = machine("on");
    let saved = Action::SetCommandLine(environment(), line("quiet loglevel=4 it's"));

    let plan = planned(&fake, &saved);

    assert_eq!(
        plan.steps,
        [
            Step {
                kind: StepKind::ImportReadWrite("tank".parse().unwrap()),
                command: None,
            },
            Step {
                kind: StepKind::SetProperty {
                    dataset: environment(),
                    property: "org.zfsbootmenu:commandline".into(),
                },
                command: Some(
                    "zfs set org.zfsbootmenu:commandline='quiet loglevel=4 it'\\''s' tank/ROOT/be"
                        .into()
                ),
            },
        ]
    );
    assert!(matches!(plan.outcome, Outcome::Done(_)), "{plan:?}");
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
    // An empty line, as its inherit, imports it first too.
    let plan = planned(&fake, &Action::SetCommandLine(environment(), line("")));
    let commands: Vec<Option<&str>> = (plan.steps.iter())
        .map(|step| step.command.as_deref())
        .collect();
    assert_eq!(
        commands,
        [
            None,
            Some("zfs inherit org.zfsbootmenu:commandline tank/ROOT/be")
        ]
    );
}

#[test]
fn the_plan_refuses_a_line_with_a_control_character_or_over_8191_bytes_with_no_step() {
    let fake = machine("on");
    for text in ["quiet\0", "quiet\nsingle", &"x".repeat(8192)] {
        let plan = planned(&fake, &Action::SetCommandLine(environment(), line(text)));

        assert!(plan.steps.is_empty(), "{plan:?}");
        let Outcome::Failed { what, .. } = &plan.outcome else {
            panic!("{plan:?}");
        };
        assert_eq!(what, "unable to save the command line of tank/ROOT/be");
    }
    assert!(changes(&fake).is_empty(), "{:?}", changes(&fake));
}
