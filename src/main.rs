use steady_state::*;
use arg::MainArg;
mod arg;

// The actor module contains all the actor implementations for this robust pipeline.
// Each actor is in its own submodule for clarity and separation of concerns.
pub(crate) mod actor {
    pub(crate) mod heartbeat;
    pub(crate) mod generator;
    pub(crate) mod worker;
    pub(crate) mod logger;
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse command-line arguments (rate, beats, etc.) using clap.
    let cli_args = MainArg::parse();

    SteadyRunner::release_build()
        .with_logging(LogLevel::Info)
        .with_telemetry_rate_ms(200) // slower telemetry frame rate, //##!##//
        .run(cli_args, move |mut graph| {

            // Construct the full actor pipeline and channel topology.
            build_graph(&mut graph);

            // Start the entire actor system. All actors and channels are now live.
            graph.start();

            // The system runs until an actor requests shutdown or the timeout is reached.
            // The timeout here is set to allow for robust failure/recovery demonstration.
            graph.block_until_stopped(Duration::from_secs(1))
        })

}

// Actor names for use in graph construction and testing.
const NAME_HEARTBEAT: &str = "HEARTBEAT";
const NAME_GENERATOR: &str = "GENERATOR";
const NAME_WORKER: &str = "WORKER";
const NAME_LOGGER: &str = "LOGGER";

/// Builds the robust actor pipeline and connects all channels.
/// This function demonstrates the robust architecture:
/// - Each actor is built with persistent state, enabling automatic restart and state recovery.
/// - Channels are created for each stage of the pipeline.
/// - Each actor is built as a SoloAct, running on its own thread for failure isolation.
fn build_graph(graph: &mut Graph) {
    let channel_builder = graph.channel_builder();


    // Create channels for each stage of the pipeline.
    let (heartbeat_tx, heartbeat_rx) = channel_builder.build();
    let (generator_tx, generator_rx) = channel_builder.build();
    let (worker_tx, worker_rx) = channel_builder.build();


    let actor_builder = graph.actor_builder()
        .with_thread_info()
        .with_load_avg()
        .with_mcpu_avg();

    // Each actor is built as a SoloAct, running on its own thread for maximum failure isolation.
    // Each actor's state is persistent and survives restarts.
    let mut shared_troupe = graph.actor_troupe();


    let state = new_state();
    actor_builder.with_name(NAME_HEARTBEAT)
        .build(move |context|
            actor::heartbeat::run(context, heartbeat_tx.clone(), state.clone())
        , MemberOf(&mut shared_troupe));

    let state = new_state();
    actor_builder.with_name(NAME_GENERATOR)
        .build(move |context|
            actor::generator::run(context, generator_tx.clone(), state.clone())
        , SoloAct);

    let state = new_state();
    actor_builder.with_name(NAME_WORKER)
        .build(move |context|
            actor::worker::run(context, heartbeat_rx.clone(), generator_rx.clone(), worker_tx.clone(), state.clone())
        , SoloAct);

    let state = new_state();
    actor_builder.with_name(NAME_LOGGER)
        .build(move |context| 
            actor::logger::run(context, worker_rx.clone(), state.clone())
        , MemberOf(&mut shared_troupe)); //same troupe as heartbeat
}

#[cfg(test)]
pub(crate) mod main_tests {
    use steady_state::*;
    use steady_state::graph_testing::*;
    use crate::actor::worker::FizzBuzzMessage;
    use super::*;

    /// This test demonstrates orchestrated, multi-actor testing using the stage manager.
    /// It allows precise control over actor behavior and verification of system interactions.
    #[test]
    fn graph_test() -> Result<(), Box<dyn Error>> {

        SteadyRunner::test_build()
            .with_logging(LogLevel::Info)
            .with_telemetry_rate_ms(200) // slower telemetry frame rate, //##!##//
            .run((), move |mut graph| {
                build_graph(&mut graph);
                graph.start();

                // Stage management provides orchestrated testing of multi-actor scenarios.
                // This enables precise control over actor behavior and verification of
                // complex system interactions without manual coordination complexity.
                let stage_manager = graph.stage_manager();
                stage_manager.actor_perform(NAME_GENERATOR, StageDirection::Echo(15u64))?;
                stage_manager.actor_perform(NAME_HEARTBEAT, StageDirection::Echo(100u64))?;
                stage_manager.actor_perform(NAME_LOGGER,    StageWaitFor::Message(FizzBuzzMessage::FizzBuzz
                                                                                  , Duration::from_secs(2)))?;
                // ...
                stage_manager.final_bow();

                graph.request_shutdown(); //essential for test to finish

                graph.block_until_stopped(Duration::from_secs(5))
            })


    }
}
