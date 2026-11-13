//! Each tier on its own thread, so filing a thought never waits for the steward, and what the
//! user asks for waits for neither.
use super::Tier;
use super::turn::{self, Stands, Turn};
use crate::knowledge::Library;
use crate::model::Session;
use crate::organizer::CloudConfig;
use std::collections::BTreeSet;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

pub struct Request {
    /// Changes whenever the user takes the map into their own hands. A turn briefed in an
    /// earlier epoch is dropped rather than rebased over what they did.
    pub epoch: u64,
    pub session: Session,
    pub holds: BTreeSet<u64>,
    /// What the user asked of the map, for a directed turn.
    pub instruction: Option<String>,
    /// The user's knowledge base as it was when the turn was asked for, when they keep one.
    pub knowledge: Option<Arc<Library>>,
}
pub struct Response {
    pub epoch: u64,
    pub tier: Tier,
    /// The request's own, so that what is journaled is what was asked.
    pub instruction: Option<String>,
    pub result: Result<Turn, String>,
}
pub struct Worker {
    pub place: UnboundedSender<Request>,
    pub steward: UnboundedSender<Request>,
    pub direct: UnboundedSender<Request>,
    pub receive: Receiver<Response>,
}
impl Worker {
    pub fn start(config: CloudConfig, wake: impl Fn() + Send + Clone + 'static) -> Self {
        let (responses, receive) = mpsc::channel();
        // What each of the user's instructions arranged, for the lanes that tend the map after.
        let asked = Arc::new(Mutex::new(Vec::<Stands>::new()));
        let lane = |tier: Tier| {
            let (send, mut requests) = unbounded_channel::<Request>();
            let (config, responses, wake) = (config.clone(), responses.clone(), wake.clone());
            let asked = asked.clone();
            std::thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                // One client for the life of the lane, on a runtime that keeps running between
                // turns: the connection stays open, so a turn costs no handshake.
                let client = config.client();
                // What the steward is not sent back to, for as long as the session is open:
                // every session starts its own lanes. `heard`: how much of what the user
                // arranged this lane has taken in.
                let (mut stands, mut heard) = (Stands::default(), 0);
                let asked = || asked.lock().unwrap_or_else(PoisonError::into_inner);
                let serve = async {
                    while let Some(mut request) = requests.recv().await {
                        // Only the newest view of the map is worth a turn. Every instruction
                        // is: each is something the user asked for.
                        while !tier.user() {
                            match requests.try_recv() {
                                Ok(newer) => request = newer,
                                Err(_) => break,
                            }
                        }
                        let result = match &client {
                            Ok(client) => {
                                let (session, holds) = (&request.session, &request.holds);
                                let wish = request.instruction.as_deref().unwrap_or_default();
                                if !tier.user() {
                                    let asked = asked();
                                    for told in &asked[heard..] {
                                        stands.absorb(&mut told.clone());
                                    }
                                    heard = asked.len();
                                }
                                let (stands, known) = (&mut stands, request.knowledge.as_deref());
                                let turn = turn::run(
                                    client, &config, tier, session, holds, wish, known, stands,
                                );
                                let turn = turn.await;
                                if tier.user() {
                                    asked().push(std::mem::take(stands));
                                }
                                turn
                            }
                            Err(error) => Err(error.clone()),
                        };
                        let response = Response {
                            epoch: request.epoch,
                            tier,
                            instruction: request.instruction,
                            result,
                        };
                        if responses.send(response).is_err() {
                            break;
                        }
                        wake();
                    }
                };
                match runtime {
                    Ok(runtime) => runtime.block_on(serve),
                    Err(_) => {
                        let result = Err("Could not start the map agent".into());
                        let _ = responses.send(Response {
                            epoch: 0,
                            tier,
                            instruction: None,
                            result,
                        });
                        wake();
                    }
                }
            });
            send
        };
        Self {
            place: lane(Tier::Place),
            steward: lane(Tier::Steward),
            direct: lane(Tier::Direct),
            receive,
        }
    }
    /// Ask a tier for a turn. False when its thread is gone.
    pub fn ask(&self, tier: Tier, request: Request) -> bool {
        match tier {
            Tier::Place => self.place.send(request).is_ok(),
            Tier::Steward => self.steward.send(request).is_ok(),
            Tier::Direct => self.direct.send(request).is_ok(),
        }
    }
}
