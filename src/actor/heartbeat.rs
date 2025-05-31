use steady_state::*;

/// HeartbeatState holds persistent state for the Heartbeat actor.
/// All fields are preserved across panics and restarts, ensuring
/// that timing and beat counts are never lost.
pub(crate) struct HeartbeatState {
    /// The current beat count.
    pub(crate) count: u64,
    /// The total number of beats sent.
    pub(crate) beats_sent: u64,
    /// Number of times this actor has restarted (for robustness tracking).
    pub(crate) restart_count: u64,
}

/// Entry point for the Heartbeat actor.
/// Demonstrates robust timing, persistent state, and automatic restart.
pub async fn run(
    context: SteadyContext,
    heartbeat_tx: SteadyTx<u64>,
    state: SteadyState<HeartbeatState>,
) -> Result<(), Box<dyn Error>> {
    let cmd = context.into_monitor([], [&heartbeat_tx]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, heartbeat_tx, state).await
    } else {
        cmd.simulated_behavior(vec!(&heartbeat_tx)).await
    }
}

/// Internal behavior for the Heartbeat actor.
/// Demonstrates robust periodic signaling and intentional failure injection.
/// State is always updated only after a successful send.
async fn internal_behavior<C: SteadyCommander>(
    mut cmd: C,
    heartbeat_tx: SteadyTx<u64>,
    state: SteadyState<HeartbeatState>,
) -> Result<(), Box<dyn Error>> {
    let args = cmd.args::<crate::MainArg>().expect("unable to downcast");
    let rate = Duration::from_millis(args.rate_ms);
    let beats = args.beats;

    let mut state = state.lock(|| HeartbeatState {
        count: 0,
        beats_sent: 0,
        restart_count: 0,
    }).await;

    // Track restarts for resilience metrics.
    state.restart_count += 1;
    info!(
        "Heartbeat starting (restart #{}) with count: {}, beats_sent: {}",
        state.restart_count, state.count, state.beats_sent
    );

    let mut heartbeat_tx = heartbeat_tx.lock().await;

    while cmd.is_running(|| heartbeat_tx.mark_closed()) {
        // Wait for both the periodic timer and channel space.
        await_for_all!(
            cmd.wait_periodic(rate),
            cmd.wait_vacant(&mut heartbeat_tx, 1)
        );

        // --- Robustness Demonstration: Intentional Panic ---
        #[cfg(not(test))]
        if state.count == 7 && state.restart_count == 1 {
            error!(
                "Heartbeat intentionally panicking at count {} to demonstrate robustness!",
                state.count
            );
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // --- End Robustness Demonstration ---

        // Prepare the beat value, attempt to send, then update state only on success.
        let beat_value = state.count;
        match cmd.try_send(&mut heartbeat_tx, beat_value) {
            SendOutcome::Success => {
                state.count += 1;
                state.beats_sent += 1;
                trace!("Heartbeat sent: {}, total beats: {}", beat_value, state.beats_sent);

                if beats == state.count {
                    info!("Heartbeat completed {} beats, requesting graph stop", beats);
                    cmd.request_shutdown().await;
                }
            }
            SendOutcome::Blocked(_) => {
                // Channel is full, try again next loop.
                continue;
            }
        }
    }

    info!(
        "Heartbeat shutting down. Final count: {}, total beats sent: {}",
        state.count, state.beats_sent
    );
    Ok(())
}

#[cfg(test)]
pub(crate) mod heartbeat_tests {
    pub use std::thread::sleep;
    use steady_state::*;
    use crate::arg::MainArg;
    use super::*;

    #[test]
    fn test_heartbeat() -> Result<(), Box<dyn Error>> {
        let mut graph = GraphBuilder::for_testing().build(MainArg {
            rate_ms: 0,
            beats: 0,
        });
        let (heartbeat_tx, heartbeat_rx) = graph.channel_builder().build();

        let state = new_state();
        graph.actor_builder()
            .with_name("UnitTest")
            .build(move |context|
                       internal_behavior(context, heartbeat_tx.clone(), state.clone())
                   , SoloAct);

        graph.start();
        sleep(Duration::from_millis(1000 * 3));
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;
        assert_steady_rx_eq_take!(&heartbeat_rx, vec!(0,1));
        Ok(())
    }
}
