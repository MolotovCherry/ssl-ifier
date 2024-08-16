use std::{borrow::Cow, collections::HashMap, sync::Arc};

use axum::{
    extract::{
        ws::{CloseFrame, Message as AMessage, WebSocket},
        Query, WebSocketUpgrade,
    },
    response::IntoResponse,
    Extension,
};
use derive_more::derive::Display;
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use owo_colors::OwoColorize;
use serde::Deserialize;
use tokio::{net::TcpStream, select};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tracing::{error, info};
use tungstenite::Message as TMessage;

use crate::{utils::format_query, StateData};

#[derive(Debug, Deserialize)]
pub struct QueryString {
    #[serde(flatten)]
    items: HashMap<String, String>,
}

pub async fn handler(
    ws: WebSocketUpgrade,
    Query(query): Query<QueryString>,
    Extension(state): Extension<Arc<StateData>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state, query))
}

async fn handle_socket(socket: WebSocket, state: Arc<StateData>, query: QueryString) {
    let (mut client_sender, client_receiver) = socket.split();

    let Some(mut url) = state.websocket_destination.clone() else {
        error!("missing destination");
        return;
    };

    // originally this would fail past an await point, but the temporary borrow drops for us and solves that.. Nice!
    url.query_pairs_mut().extend_pairs(query.items).finish();

    let path = url.path();
    let query = format_query(url.query().unwrap_or(""));

    let path = format!("{path}{query}");
    let path = path.cyan();

    info!(url = %path, "connecting to ws");

    let dest_socket = {
        let Ok((dest, _)) = connect_async(url.as_str()).await else {
            // failed to connect to destination, so the client connection isn't needed

            error!("failed to connect");

            let frame = CloseFrame {
                // Bad Gateway
                code: 1014,
                reason: Cow::Borrowed("Failed to open connection to destination server"),
            };

            _ = client_sender.send(AMessage::Close(Some(frame))).await;

            _ = client_sender.close().await;

            return;
        };

        dest
    };

    let (dest_sender, dest_receiver) = dest_socket.split();

    let client_fut = handle_from_client(client_receiver, dest_sender);
    let dest_fut = handle_from_server(client_sender, dest_receiver);

    // whichever future completes first, abort the other one since they're a pair
    select! {
        _ = client_fut => (),
        _ = dest_fut => ()
    }
}

async fn handle_from_client(
    mut client_receiver: SplitStream<WebSocket>,
    mut dest_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TMessage>,
) {
    while let Some(Ok(msg)) = client_receiver.next().await {
        let msg = into_tmessage(msg);

        info!(ty = %msg_ty(&msg), %msg, "client->server");

        if dest_sender.send(msg).await.is_err() {
            break;
        }
    }
}

async fn handle_from_server(
    mut client_sender: SplitSink<WebSocket, AMessage>,
    mut dest_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) {
    while let Some(Ok(msg)) = dest_receiver.next().await {
        info!(ty = %msg_ty(&msg), %msg, "server->client");

        let Some(msg) = into_amessage(msg) else {
            continue;
        };

        if client_sender.send(msg).await.is_err() {
            break;
        }
    }
}

fn into_tmessage(msg: AMessage) -> TMessage {
    use tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};

    match msg {
        AMessage::Text(t) => TMessage::Text(t),
        AMessage::Binary(b) => TMessage::Binary(b),
        AMessage::Ping(p) => TMessage::Ping(p),
        AMessage::Pong(p) => TMessage::Pong(p),
        AMessage::Close(c) => match c {
            Some(frame) => {
                let frame = CloseFrame {
                    code: CloseCode::from(frame.code),
                    reason: frame.reason,
                };

                TMessage::Close(Some(frame))
            }

            None => TMessage::Close(None),
        },
    }
}

fn into_amessage(msg: TMessage) -> Option<AMessage> {
    use axum::extract::ws::CloseCode;

    let msg = match msg {
        TMessage::Text(t) => AMessage::Text(t),
        TMessage::Binary(b) => AMessage::Binary(b),
        TMessage::Ping(p) => AMessage::Ping(p),
        TMessage::Pong(p) => AMessage::Pong(p),
        TMessage::Close(c) => match c {
            Some(frame) => {
                let frame = CloseFrame {
                    code: CloseCode::from(frame.code),
                    reason: frame.reason,
                };

                AMessage::Close(Some(frame))
            }

            None => AMessage::Close(None),
        },

        TMessage::Frame(_) => return None,
    };

    Some(msg)
}

fn msg_ty(msg: &TMessage) -> PrintableMessage {
    msg.into()
}

#[derive(Debug, Display)]
enum PrintableMessage {
    Text,
    Binary,
    Ping,
    Pong,
    Close,
    Frame,
}

impl From<&TMessage> for PrintableMessage {
    fn from(value: &TMessage) -> Self {
        match value {
            TMessage::Text(_) => Self::Text,
            TMessage::Binary(_) => Self::Binary,
            TMessage::Ping(_) => Self::Ping,
            TMessage::Pong(_) => Self::Pong,
            TMessage::Close(_) => Self::Close,
            TMessage::Frame(_) => Self::Frame,
        }
    }
}
