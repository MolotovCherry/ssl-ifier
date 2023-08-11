use crate::StateData;

use axum::{
    extract::{
        ws::{CloseFrame as ACloseFrame, Message as AMessage, WebSocket, WebSocketUpgrade},
        Query,
    },
    response::IntoResponse,
    Extension,
};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tungstenite::protocol::{
    frame::coding::CloseCode, CloseFrame as TCloseFrame, Message as TMessage,
};

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

    let mut url = state.websocket_destination.as_ref().unwrap().to_owned();
    // originally this would fail past an await point, but the temporary borrow drops for us and solves that.. Nice!
    url.query_pairs_mut().extend_pairs(query.items).finish();

    let dest_socket = {
        if let Ok((dest_socket, _)) = connect_async(url.as_str()).await {
            dest_socket
        } else {
            // failed to connect to destination, so the client connection isn't needed
            let _ = client_sender.close().await;
            return;
        }
    };

    let (dest_sender, dest_reader) = dest_socket.split();

    tokio::spawn(handle_from_dest(client_sender, dest_reader));
    tokio::spawn(handle_from_client(client_receiver, dest_sender));
}

async fn handle_from_client(
    mut client_receiver: SplitStream<WebSocket>,
    mut dest_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, TMessage>,
) {
    while let Some(Ok(msg)) = client_receiver.next().await {
        let message = match msg {
            AMessage::Text(text) => TMessage::Text(text),

            AMessage::Binary(binary) => TMessage::Binary(binary),

            AMessage::Ping(ping) => TMessage::Ping(ping),

            AMessage::Pong(pong) => TMessage::Pong(pong),

            AMessage::Close(Some(close)) => TMessage::Close(Some(TCloseFrame {
                code: CloseCode::from(close.code),
                reason: close.reason,
            })),

            AMessage::Close(None) => TMessage::Close(None),
        };

        if dest_sender.send(message).await.is_err() {
            let _ = dest_sender.close().await;
            return;
        }
    }
}

async fn handle_from_dest(
    mut client_sender: SplitSink<WebSocket, AMessage>,
    mut dest_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) {
    while let Some(Ok(msg)) = dest_receiver.next().await {
        let message = match msg {
            TMessage::Text(text) => AMessage::Text(text),

            TMessage::Binary(binary) => AMessage::Binary(binary),

            TMessage::Ping(ping) => AMessage::Ping(ping),

            TMessage::Pong(pong) => AMessage::Pong(pong),

            TMessage::Close(Some(close)) => AMessage::Close(Some(ACloseFrame {
                code: close.code.into(),
                reason: close.reason,
            })),

            TMessage::Close(None) => AMessage::Close(None),

            // we can ignore `Frame` frames as recommended by the tungstenite maintainers
            // https://github.com/snapview/tungstenite-rs/issues/268
            TMessage::Frame(_) => continue,
        };

        if client_sender.send(message).await.is_err() {
            let _ = client_sender.close().await;
            return;
        }
    }
}
