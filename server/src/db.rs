use actix::prelude::*;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::result::Error as DError;
use dotenv::dotenv;
use std::env;

use crate::schema::users;
use crate::schema::games;

fn establish_connection() -> PgConnection {
    dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgConnection::establish(&database_url).expect(&format!("Error connecting to {}", database_url))
}

#[derive(Queryable)]
pub struct User {
    pub id: i64,
    pub auth_token: String,
    pub nick: Option<String>,
}

#[derive(Insertable, AsChangeset)]
#[table_name = "users"]
pub struct NewUser<'a> {
    pub auth_token: &'a str,
    pub nick: Option<&'a str>,
}

pub struct IdentifyUser {
    pub auth_token: String,
    pub nick: Option<String>,
}

impl Message for IdentifyUser {
    type Result = Result<User, ()>;
}

#[derive(Queryable, Debug)]
pub struct Game {
    pub id: i64,
    pub name: String,
    pub replay: Option<Vec<u8>>,
}

#[derive(Insertable, AsChangeset)]
#[table_name = "games"]
pub struct NewGame<'a> {
    pub id: Option<i64>,
    pub name: &'a str,
    pub replay: Option<&'a [u8]>,
}

pub struct StoreGame {
    pub id: Option<u64>,
    pub name: String,
    pub replay: Option<Vec<u8>>,
}

impl Message for StoreGame {
    type Result = Result<Game, ()>;
}

pub struct GetGame(pub u64);

impl Message for GetGame {
    type Result = Result<Game, ()>;
}

pub struct DbActor {
    connection: PgConnection,
}

impl Default for DbActor {
    fn default() -> Self {
        DbActor {
            connection: establish_connection(),
        }
    }
}

impl Actor for DbActor {
    type Context = SyncContext<Self>;

    fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
        println!("Database actor stopping!");

        Running::Stop
    }
}

impl Handler<IdentifyUser> for DbActor {
    type Result = Result<User, ()>;

    fn handle(&mut self, msg: IdentifyUser, _ctx: &mut Self::Context) -> Self::Result {
        use crate::schema::users::dsl::*;

        let new_user = NewUser {
            auth_token: &msg.auth_token,
            nick: msg.nick.as_ref().map(|x| &**x),
        };

        let existing = users
            .filter(auth_token.eq(&msg.auth_token))
            .first::<User>(&self.connection);

        let result = match existing {
            Ok(u) => diesel::update(users.filter(id.eq(u.id)))
                .set(new_user)
                .get_result(&self.connection),
            Err(DError::NotFound) => diesel::insert_into(users)
                .values(new_user)
                .get_result(&self.connection),
            Err(e) => {
                println!("{:?}", e);
                return Err(());
            }
        };

        result.map_err(|_| ())
    }
}

impl Handler<StoreGame> for DbActor {
    type Result = Result<Game, ()>;

    fn handle(&mut self, msg: StoreGame, _ctx: &mut Self::Context) -> Self::Result {
        use crate::schema::games::dsl::*;

        let new_game = NewGame {
            id: msg.id.map(|x| x as _),
            name: &msg.name,
            replay: msg.replay.as_ref().map(|x| &**x),
        };

        let result = match msg.id {
            Some(m_id) => diesel::update(games.filter(id.eq(m_id as i64)))
                .set(new_game)
                .get_result(&self.connection),
            None => diesel::insert_into(games)
                .values(new_game)
                .get_result(&self.connection),
        };

        result.map_err(|e| {
                println!("{:?}", e);
        })
    }
}

impl Handler<GetGame> for DbActor {
    type Result = Result<Game, ()>;

    fn handle(&mut self, msg: GetGame, _ctx: &mut Self::Context) -> Self::Result {
        use crate::schema::games::dsl::*;

        let result = games.find(msg.0 as i64).first(&self.connection);

        result.map_err(|e| {
                println!("{:?}", e);
        })
    }
}