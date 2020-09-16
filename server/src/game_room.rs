use actix::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::{db, server};
use shared::game;
use shared::message;

// TODO: add room timeout

///////////////////////////////////////////////////////////////////////////////
//                               Actor messages                              //
///////////////////////////////////////////////////////////////////////////////

// Output /////////////////////////////////////////////////////////////////////

#[derive(Message, Clone)]
#[rtype(result = "()")]
pub enum Message {
    // TODO: Use a proper struct, not magic tuples
    GameStatus {
        room_id: u32,
        members: Vec<u64>,
        view: game::GameView,
    },
    BoardAt {
        room_id: u32,
        view: game::GameHistory,
    },
}

// Actions ////////////////////////////////////////////////////////////////////

pub struct GameAction {
    pub id: usize,
    pub action: message::GameAction,
}

impl actix::Message for GameAction {
    type Result = Result<(), message::Error>;
}

// User lifecycle /////////////////////////////////////////////////////////////

#[derive(Message)]
#[rtype(result = "()")]
pub struct Leave {
    pub session_id: usize,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct Join {
    pub session_id: usize,
    pub user_id: u64,
    pub addr: Recipient<Message>,
}

// Control ////////////////////////////////////////////////////////////////////

#[derive(Message)]
#[rtype(result = "()")]
pub struct Unload;

///////////////////////////////////////////////////////////////////////////////
//                                   Actor                                   //
///////////////////////////////////////////////////////////////////////////////

pub struct GameRoom {
    pub room_id: u32,
    pub sessions: HashMap<usize, (u64, Recipient<Message>)>,
    pub users: HashSet<u64>,
    pub name: String,
    pub last_action: Instant,
    pub game: game::Game,
    pub db: Addr<db::DbActor>,
    pub server: Addr<server::GameServer>,
}

impl GameRoom {
    fn send_room_messages(&self, mut create_msg: impl FnMut(u64) -> Message) {
        for (user_id, addr) in self.sessions.values() {
            let _ = addr.do_send(create_msg(*user_id));
        }
    }
}

impl Actor for GameRoom {
    type Context = Context<Self>;

    fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
        println!("Room {} stopping!", self.room_id);

        Running::Stop
    }
}

impl Handler<Leave> for GameRoom {
    type Result = ();

    fn handle(&mut self, msg: Leave, _ctx: &mut Self::Context) -> Self::Result {
        let Leave { session_id } = msg;

        if let Some((user_id, _addr)) = self.sessions.remove(&session_id) {
            let sessions = &self.sessions;
            if !sessions.values().any(|(uid, _addr)| *uid == user_id) {
                self.users.remove(&user_id);
                self.send_room_messages(|user_id| Message::GameStatus {
                    room_id: self.room_id,
                    members: self.users.iter().copied().collect(),
                    view: self.game.get_view(user_id),
                });
            }
        }
    }
}

impl Handler<Join> for GameRoom {
    type Result = ();

    fn handle(&mut self, msg: Join, _ctx: &mut Self::Context) -> Self::Result {
        let Join {
            session_id,
            user_id,
            addr,
        } = msg;

        self.sessions.insert(session_id, (user_id, addr));
        self.users.insert(user_id);
        self.send_room_messages(|user_id| Message::GameStatus {
            room_id: self.room_id,
            members: self.users.iter().copied().collect(),
            view: self.game.get_view(user_id),
        });

        // TODO: Announce profile to room members

        // Broadcast the profile of each seatholder
        // .. this is not great
        for seat in &self.game.shared.seats {
            if let Some(user_id) = seat.player {
                self.server.do_send(server::QueryProfile { user_id });
            }
        }
    }
}

impl Handler<GameAction> for GameRoom {
    type Result = MessageResult<GameAction>;

    fn handle(&mut self, msg: GameAction, _: &mut Context<Self>) -> MessageResult<GameAction> {
        use message::Error;

        let GameAction { id, action } = msg;

        let &(user_id, ref addr) = match self.sessions.get(&id) {
            Some(x) => x,
            None => return MessageResult(Err(Error::other("No session"))),
        };

        self.last_action = Instant::now();
        let res = match action {
            message::GameAction::Place(x, y) => self
                .game
                .make_action(user_id, game::ActionKind::Place(x, y))
                .map_err(Into::into),
            message::GameAction::Pass => self
                .game
                .make_action(user_id, game::ActionKind::Pass)
                .map_err(Into::into),
            message::GameAction::Cancel => self
                .game
                .make_action(user_id, game::ActionKind::Cancel)
                .map_err(Into::into),
            message::GameAction::TakeSeat(seat_id) => self
                .game
                .take_seat(user_id, seat_id as _)
                .map_err(Into::into),
            message::GameAction::LeaveSeat(seat_id) => self
                .game
                .leave_seat(user_id, seat_id as _)
                .map_err(Into::into),
            message::GameAction::BoardAt(start, end) => {
                if start > end {
                    return MessageResult(Ok(()));
                }
                // Prevent asking for a ridiculous amount.
                if end as usize > self.game.shared.board_history.len() + 20 {
                    return MessageResult(Ok(()));
                }
                for turn in (start..=end).rev() {
                    let view = self.game.get_view_at(user_id, turn);
                    if let Some(view) = view {
                        let _ = addr.do_send(Message::BoardAt {
                            room_id: self.room_id,
                            view,
                        });
                    }
                }
                return MessageResult(Ok(()));
            }
        };

        if let Err(err) = res {
            return MessageResult(Err(Error::Game {
                room_id: self.room_id,
                error: err,
            }));
        }

        self.db.do_send(db::StoreGame {
            id: Some(self.room_id as _),
            name: self.name.clone(),
            replay: Some(self.game.dump()),
        });

        self.send_room_messages(|user_id| Message::GameStatus {
            room_id: self.room_id,
            members: self.users.iter().copied().collect(),
            view: self.game.get_view(user_id),
        });

        MessageResult(Ok(()))
    }
}

impl Handler<Unload> for GameRoom {
    type Result = ();

    fn handle(&mut self, _: Unload, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
    }
}
