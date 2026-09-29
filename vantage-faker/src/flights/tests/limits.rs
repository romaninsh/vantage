use super::*;

#[test]
fn validate_rejects_oversized_fleet_and_board() {
    let at_max = FlightsConfig {
        fleet: FlightsConfig::MAX_FLEET,
        board: FlightsConfig::MAX_BOARD,
        ..FlightsConfig::default()
    };
    assert_eq!(at_max.validate(), Ok(()));

    let fleet = FlightsConfig {
        fleet: 400_000,
        ..FlightsConfig::default()
    };
    let err = fleet.validate().unwrap_err();
    assert!(
        err.contains("fleet 400000") && err.contains("2000"),
        "{err}"
    );

    let board = FlightsConfig {
        board: 501,
        ..FlightsConfig::default()
    };
    let err = board.validate().unwrap_err();
    assert!(err.contains("board 501") && err.contains("500"), "{err}");
}

#[test]
fn new_clamps_an_unvalidated_config() {
    let fx = FlightsEffect::new(FlightsConfig {
        fleet: 400_000,
        board: 400_000,
        seed: Some(9),
        ..FlightsConfig::default()
    });
    assert_eq!(fx.cfg.fleet, FlightsConfig::MAX_FLEET);
    assert_eq!(fx.cfg.board, FlightsConfig::MAX_BOARD);

    let (ctx, _rx) = ctx(columns(&["id", "phase"]));
    fx.seed(&ctx);
    assert_eq!(
        ctx.record_count(),
        FlightsConfig::MAX_FLEET + FlightsConfig::MAX_BOARD
    );
}

#[test]
fn top_up_fills_a_large_board_in_one_call() {
    let board = FlightsConfig::MAX_BOARD;
    let mut sim = FlightSim::new(0, board, Some(10), T0);
    assert_eq!(sim.flights.len(), board);
    assert!(sim.top_up(T0).is_empty());

    let later = T0 + 6.0 * 3600.0;
    let departed = sim.count(later, |p| p > Phase::Boarding);
    assert!(departed > 0);
    assert_eq!(sim.top_up(later).len(), departed);
    assert_eq!(sim.count(later, |p| p <= Phase::Boarding), board);
}
