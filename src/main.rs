use steady_state::*;
use arg::MainArg;
mod arg;

pub(crate) mod actor {
    pub(crate) mod heartbeat;
    pub(crate) mod generator;
    pub(crate) mod worker;
    pub(crate) mod logger;
}

fn main() -> Result<(), Box<dyn Error>> {

    let cli_args = MainArg::parse();
    let _ = init_logging(LogLevel::Info);
    let mut graph = GraphBuilder::default()
        .build(cli_args);

    build_graph(&mut graph);

    //startup entire graph
    graph.start();
    // your graph is running here until actor calls graph stop
    graph.block_until_stopped(Duration::from_secs(1))
}

const NAME_HEARTBEAT: &str = "heartbeat";
const NAME_GENERATOR: &str = "generator";
const NAME_WORKER: &str = "worker";
const NAME_LOGGER: &str = "logger";

fn build_graph(graph: &mut Graph) {
    let channel_builder = graph.channel_builder();

    let (heartbeat_tx, heartbeat_rx) = channel_builder.build();
    let (generator_tx, generator_rx) = channel_builder.build();
    let (worker_tx, worker_rx) = channel_builder.build();

    // Enable actor restarts for robustness
    let actor_builder = graph.actor_builder()
        .with_mcpu_avg(); // This enables automatic restart on panic

    let state = new_state();
    actor_builder.with_name(NAME_HEARTBEAT)
        .build(move |context| {
            actor::heartbeat::run(context, heartbeat_tx.clone(), state.clone())
        }, &mut Threading::Spawn);

    let state = new_state();
    actor_builder.with_name(NAME_GENERATOR)
        .build(move |context| {
            actor::generator::run(context, generator_tx.clone(), state.clone())
        }, &mut Threading::Spawn);

    let state = new_state();
    actor_builder.with_name(NAME_WORKER)
        .build(move |context| {
            actor::worker::run(context, heartbeat_rx.clone(), generator_rx.clone(), worker_tx.clone(), state.clone())
        }, &mut Threading::Spawn);

    let state = new_state();
    actor_builder.with_name(NAME_LOGGER)
        .build(move |context| {
            actor::logger::run(context, worker_rx.clone(), state.clone())
        }, &mut Threading::Spawn);
}

//TODO: also must demo the DLQ when we pick up the same failure mesage multipel times !!!

#[cfg(test)]
pub(crate) mod main_tests {
    use steady_state::*;
    use steady_state::graph_testing::{StageDirection, StageWaitFor};
    use crate::actor::worker::FizzBuzzMessage;
    use super::*;

    #[test]
    fn graph_test() -> Result<(), Box<dyn Error>> {

        let mut graph = GraphBuilder::for_testing()
            .build(MainArg::default());

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
        stage_manager.final_bow();

        graph.request_shutdown();

        graph.block_until_stopped(Duration::from_secs(5))

    }
}
