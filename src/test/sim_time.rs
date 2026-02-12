use crate::sim::SimTime;

#[test]
fn sim_time_unit_conversions() {
    assert_eq!(SimTime::from_micros(1), SimTime(1_000));
    assert_eq!(SimTime::from_millis(1), SimTime(1_000_000));
    assert_eq!(SimTime::from_secs(1), SimTime(1_000_000_000));
}

#[test]
fn sim_time_unit_conversions_saturate_on_overflow() {
    assert_eq!(SimTime::from_micros(u64::MAX), SimTime(u64::MAX));
    assert_eq!(SimTime::from_millis(u64::MAX), SimTime(u64::MAX));
    assert_eq!(SimTime::from_secs(u64::MAX), SimTime(u64::MAX));
}
