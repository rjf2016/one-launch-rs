use std::sync::{Arc, Mutex};

use ol_platform_api::{RiotLaunchError, RiotLaunchRequest, RiotLocalLaunch};

use crate::OpLog;

type Play = Arc<dyn Fn(&str) -> Result<(), RiotLaunchError> + Send + Sync>;

// Sends nothing and reads no lockfile: a launch request is written down by its product id and handed to the scripted client, which takes it as a press of its Play button.
pub struct FakeRiotLaunch {
    log: OpLog,
    client: Mutex<Option<Play>>,
}

impl FakeRiotLaunch {
    pub fn new(log: OpLog) -> Self {
        Self {
            log,
            client: Mutex::new(None),
        }
    }

    // The scripted client hears each request by `<product>.<patchline>` and says whether it took it.
    pub fn on_launch(
        &self,
        play: impl Fn(&str) -> Result<(), RiotLaunchError> + Send + Sync + 'static,
    ) {
        *self.client() = Some(Arc::new(play));
    }

    fn client(&self) -> std::sync::MutexGuard<'_, Option<Play>> {
        self.client
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl RiotLocalLaunch for FakeRiotLaunch {
    fn launch(&self, request: &RiotLaunchRequest<'_>) -> Result<(), RiotLaunchError> {
        let Some(play) = self.client().clone() else {
            return Err(RiotLaunchError::NoLockfile);
        };
        let id = format!("{}.{}", request.product, request.patchline);
        self.log.record("riot.launch", [("id", id.clone().into())]);
        play(&id)
    }
}

#[cfg(test)]
mod tests {
    use ol_platform_api::CancelToken;
    use serde_json::json;

    use super::*;

    #[test]
    fn a_request_is_written_down_by_its_product_and_goes_to_the_scripted_client() {
        let log = OpLog::new();
        let fake = FakeRiotLaunch::new(log.clone());
        let cancel = CancelToken::new();
        let request = RiotLaunchRequest {
            product: "valorant",
            patchline: "live",
            lockfile_path: "lockfile",
            cancel: &cancel,
        };
        assert_eq!(fake.launch(&request), Err(RiotLaunchError::NoLockfile));
        assert!(log.snapshot().is_empty());

        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&heard);
        fake.on_launch(move |id| {
            sink.lock().unwrap().push(id.to_owned());
            Err(RiotLaunchError::Status(500))
        });
        assert_eq!(fake.launch(&request), Err(RiotLaunchError::Status(500)));
        assert_eq!(*heard.lock().unwrap(), ["valorant.live"]);
        assert_eq!(
            log.snapshot(),
            [json!({ "k": "plat", "op": "riot.launch", "id": "valorant.live" })]
        );
    }
}
