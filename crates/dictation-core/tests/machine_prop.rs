//! Propriedade da máquina de estados: para qualquer sequência de eventos,
//! ela nunca entra num estado inválido nem muda de estado numa transição rejeitada.

use dictation_core::{Event, Machine, Phase};
use proptest::prelude::*;

fn ev(n: u8) -> Event {
    match n % 3 {
        0 => Event::Start,
        1 => Event::Stop,
        _ => Event::Finished,
    }
}

proptest! {
    #[test]
    fn machine_never_breaks_invariants(events in prop::collection::vec(0u8..3, 0..200)) {
        let mut m = Machine::new();
        for e in events {
            let event = ev(e);
            let before = m.phase();
            match m.on(event) {
                Ok(_) => {
                    match (before, event) {
                        (Phase::Idle, Event::Start) => {
                            prop_assert_eq!(m.phase(), Phase::Recording);
                            prop_assert!(m.session().is_some());
                        }
                        (Phase::Recording, Event::Stop) => {
                            prop_assert_eq!(m.phase(), Phase::Finalizing);
                        }
                        (Phase::Finalizing, Event::Finished) => {
                            prop_assert_eq!(m.phase(), Phase::Idle);
                            prop_assert!(m.session().is_none());
                        }
                        _ => prop_assert!(false, "transição inválida foi aceita: {:?} -> {:?}", before, event),
                    }
                }
                Err(_) => {
                    prop_assert_eq!(m.phase(), before, "estado não pode mudar em transição rejeitada");
                }
            }
        }
    }
}
