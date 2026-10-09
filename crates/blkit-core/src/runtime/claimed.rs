use super::*;

pub(crate) async fn execute_claimed(
    graph: Arc<GraphDefinition>,
    claimed: DistributedInstance,
    store: PostgresStore,
    worker_id: String,
    permits: Arc<Semaphore>,
    definitions: Arc<NamedRegistry>,
) -> Result<(), String> {
    if claimed.instance.status == "cancelled" {
        return Ok(());
    }
    if claimed.owner_id.as_deref() != Some(worker_id.as_str()) {
        return Err("instance is not owned by this worker".into());
    }
    let context = Context {
        state: Arc::new(Mutex::new(Running {
            status: "running",
            next: 0,
            in_flight: HashMap::new(),
        })),
        #[cfg(feature = "local-persistence")]
        store: None,
        claim: Some(Claim {
            store: store.clone(),
            worker_id: worker_id.clone(),
            generation: claimed.generation,
        }),
        id: claimed.instance.id.clone(),
    };
    let (input, checkpoint) = if let Some(checkpoint) = claimed.instance.checkpoint {
        checkpoint.ensure_supported()?;
        (claimed.instance.input.clone(), checkpoint)
    } else {
        let prepared = (|| {
            let input = (graph.decode_input)(claimed.instance.input.clone())
                .map_err(|error| format!("invalid input: {error}"))?;
            let mut checkpoint = graph.checkpoint(&input)?;
            graph.resume_due(&input, &mut checkpoint, crate::store::now_ms())?;
            Ok::<_, String>((input, checkpoint))
        })();
        let (input, checkpoint) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                store
                    .finish_owned(
                        &context.id,
                        &worker_id,
                        claimed.generation,
                        "failed",
                        None,
                        None,
                        Some(&error),
                    )
                    .await?;
                return Ok(());
            }
        };
        if !store
            .commit_initial_checkpoint(
                &context.id,
                &worker_id,
                claimed.generation,
                &input,
                &checkpoint,
            )
            .await?
        {
            return Err("lost claim before initializing checkpoint".into());
        }
        (input, checkpoint)
    };
    let watch = async {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            match store
                .claim_activity(&context.id, &worker_id, claimed.generation)
                .await?
            {
                ClaimActivity::Active => {}
                ClaimActivity::Cancelled => {
                    context.signal_cancel().await;
                    return Ok::<bool, String>(true);
                }
                ClaimActivity::Lost => {
                    context.signal_cancel().await;
                    return Ok(false);
                }
            }
        }
    };
    let outcome = tokio::select! {
        result = execute_named(&graph, &input, checkpoint, &permits, &context, &definitions) => result,
        cancelled = watch => {
            return if cancelled? { Ok(()) } else { Err("lost claim during execution".into()) };
        }
    };
    match outcome {
        Ok(NamedOutcome::Completed(value)) => context.complete(value).await,
        Ok(NamedOutcome::Terminal(terminal)) => context.named_terminal(&terminal).await,
        Ok(NamedOutcome::Waiting(_)) => Ok(()),
        Err(error) => {
            if store
                .get(&context.id)
                .await?
                .is_some_and(|row| row.instance.status == "cancelled")
            {
                context.signal_cancel().await;
                Ok(())
            } else {
                context
                    .fail_attempt(graph.retry.as_ref(), &error)
                    .await
                    .map(|_| ())
            }
        }
    }
}
