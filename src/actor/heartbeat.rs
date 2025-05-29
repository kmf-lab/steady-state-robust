use steady_state::*;

/// by keeping the count in steady state this will not be lost or reset if this actor should panic
pub(crate) struct HeartbeatState {
    pub(crate) count: u64,
    pub(crate) beats_sent: u64,
    pub(crate) restart_count: u64,  // Track how many times we've restarted
}

/// this is the normal entry point for our actor in the graph using its normal implementation
pub async fn run(context: SteadyContext, heartbeat_tx: SteadyTx<u64>, state: SteadyState<HeartbeatState>) -> Result<(),Box<dyn Error>> {
    let cmd = context.into_monitor([], [&heartbeat_tx]);
    if cmd.use_internal_behavior {
        internal_behavior(cmd, heartbeat_tx, state).await
    } else {
        cmd.simulated_behavior(vec!(&heartbeat_tx)).await
    }
}

async fn internal_behavior<C: SteadyCommander>(mut cmd: C
                                               , heartbeat_tx: SteadyTx<u64>
                                               , state: SteadyState<HeartbeatState> ) -> Result<(),Box<dyn Error>> {
    let args = cmd.args::<crate::MainArg>().expect("unable to downcast");
    let rate = Duration::from_millis(args.rate_ms);
    let beats = args.beats;

    let mut state = state.lock(|| HeartbeatState{
        count: 0,
        beats_sent: 0,
        restart_count: 0,
    }).await;

    // Increment restart count to track actor resilience
    state.restart_count += 1;
    info!("Heartbeat starting (restart #{}) with count: {}, beats_sent: {}", 
          state.restart_count, state.count, state.beats_sent);

    let mut heartbeat_tx = heartbeat_tx.lock().await;

    //loop is_running until shutdown signal then we call the closure which closes our outgoing Tx
    while cmd.is_running(|| heartbeat_tx.mark_closed()) {
        //await here until both conditions are met
        await_for_all!(cmd.wait_periodic(rate),
                       cmd.wait_vacant(&mut heartbeat_tx, 1));

        // ROBUSTNESS DEMONSTRATION: Intentional panic for testing
        // NOTE: Future engineers should NEVER include panic conditions like this in production code!
        // This is only for demonstrating the framework's recovery capabilities.
        #[cfg(not(test))]  //do not throw in tests only at runtime.
        if state.count == 7 && state.restart_count == 1 {
            error!("Heartbeat intentionally panicking at count {} to demonstrate robustness!", state.count);
            panic!("Intentional panic for robustness demonstration - DO NOT COPY THIS PATTERN!");
        }
        // END ROBUSTNESS DEMONSTRATION

        // Robust pattern: Prepare the beat value, attempt to send, then update state only on success
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
                // Channel is full, continue to next iteration to wait for space
                continue;
            }
        }
    }

    info!("Heartbeat shutting down. Final count: {}, total beats sent: {}", state.count, state.beats_sent);
    Ok(())
}

/// Here we test the internal behavior of this actor
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
            .build_spawn(move |context|
                internal_behavior(context, heartbeat_tx.clone(), state.clone())
            );

        graph.start();
        sleep(Duration::from_millis(1000 * 3));
        graph.request_shutdown();
        graph.block_until_stopped(Duration::from_secs(1))?;
        assert_steady_rx_eq_take!(&heartbeat_rx, vec!(0,1));
        Ok(())
    }
}
