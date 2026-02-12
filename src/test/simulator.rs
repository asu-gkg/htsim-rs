use crate::sim::{Event, SimTime, Simulator, World};
use std::any::Any;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct DummyWorld {
    ticks: usize,
}

impl World for DummyWorld {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn on_tick(&mut self, _sim: &mut Simulator) {
        self.ticks = self.ticks.saturating_add(1);
    }
}

struct Push {
    id: u32,
    log: Arc<Mutex<Vec<u32>>>,
}

impl Event for Push {
    fn execute(self: Box<Self>, _sim: &mut Simulator, _world: &mut dyn World) {
        let Push { id, log } = *self;
        log.lock().expect("log lock").push(id);
    }
}

struct PushThenScheduleNow {
    id: u32,
    next_id: u32,
    log: Arc<Mutex<Vec<u32>>>,
}

impl Event for PushThenScheduleNow {
    fn execute(self: Box<Self>, sim: &mut Simulator, _world: &mut dyn World) {
        let PushThenScheduleNow { id, next_id, log } = *self;
        log.lock().expect("log lock").push(id);
        sim.schedule(sim.now(), Push { id: next_id, log });
    }
}

#[test]
fn scheduled_events_order_by_time_then_seq() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let mut sim = Simulator::default();
    sim.schedule(
        SimTime(10),
        Push {
            id: 1,
            log: Arc::clone(&log),
        },
    );
    sim.schedule(
        SimTime(5),
        Push {
            id: 2,
            log: Arc::clone(&log),
        },
    );
    sim.schedule(
        SimTime(10),
        Push {
            id: 3,
            log: Arc::clone(&log),
        },
    );

    let mut world = DummyWorld::default();
    sim.run(&mut world);

    assert_eq!(&*log.lock().expect("log lock"), &[2, 1, 3]);
    assert_eq!(world.ticks, 3);
    assert_eq!(sim.now(), SimTime(10));
}

#[test]
fn event_scheduled_at_same_time_inside_event_runs_after_current_event() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let mut sim = Simulator::default();
    sim.schedule(
        SimTime::ZERO,
        PushThenScheduleNow {
            id: 1,
            next_id: 2,
            log: Arc::clone(&log),
        },
    );

    let mut world = DummyWorld::default();
    sim.run(&mut world);

    assert_eq!(&*log.lock().expect("log lock"), &[1, 2]);
    assert_eq!(world.ticks, 2);
    assert_eq!(sim.now(), SimTime::ZERO);
}

#[test]
fn run_until_skips_events_after_until_and_advances_time() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let mut sim = Simulator::default();
    sim.schedule(
        SimTime::ZERO,
        Push {
            id: 1,
            log: Arc::clone(&log),
        },
    );
    sim.schedule(
        SimTime(10),
        Push {
            id: 2,
            log: Arc::clone(&log),
        },
    );

    let mut world = DummyWorld::default();
    sim.run_until(SimTime(5), &mut world);

    assert_eq!(&*log.lock().expect("log lock"), &[1]);
    assert_eq!(world.ticks, 1);
    assert_eq!(sim.now(), SimTime(5));

    sim.run(&mut world);
    assert_eq!(&*log.lock().expect("log lock"), &[1, 2]);
    assert_eq!(world.ticks, 2);
    assert_eq!(sim.now(), SimTime(10));
}

#[test]
fn run_until_executes_events_scheduled_exactly_at_until() {
    let log = Arc::new(Mutex::new(Vec::new()));

    let mut sim = Simulator::default();
    sim.schedule(
        SimTime(5),
        Push {
            id: 1,
            log: Arc::clone(&log),
        },
    );

    let mut world = DummyWorld::default();
    sim.run_until(SimTime(5), &mut world);

    assert_eq!(&*log.lock().expect("log lock"), &[1]);
    assert_eq!(world.ticks, 1);
    assert_eq!(sim.now(), SimTime(5));
}

#[test]
fn run_until_advances_time_even_if_there_are_no_events() {
    let mut sim = Simulator::default();
    let mut world = DummyWorld::default();

    sim.run_until(SimTime(7), &mut world);
    assert_eq!(sim.now(), SimTime(7));
    assert_eq!(world.ticks, 0);
}
