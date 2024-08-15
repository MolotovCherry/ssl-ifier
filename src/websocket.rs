use std::{borrow::Cow, collections::HashMap, sync::Arc};

use axum::{extract::Query, response::IntoResponse, Extension};
use axum_tungstenite::{WebSocket, WebSocketUpgrade};
use futures::{
    sink::SinkExt,
    stream::{SplitSink, SplitStream, StreamExt},
};
use serde::Deserialize;
use tokio::{net::TcpStream, select};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tungstenite::protocol::{frame::coding::CloseCode, CloseFrame, Message};

use crate::StateData;

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
            let _ = client_sender
                .send(Message::Close(Some(CloseFrame {
                    // Bad Gateway
                    code: CloseCode::from(1014),
                    reason: Cow::Borrowed("Failed to open connection to destination server"),
                })))
                .await;

            let _ = client_sender.close().await;
            return;
        }
    };

    let (dest_sender, dest_receiver) = dest_socket.split();

    let client_fut = handle_from_client(client_receiver, dest_sender);
    let dest_fut = handle_from_dest(client_sender, dest_receiver);

    // whichever future completes first, abort the other one since they're a pair
    select! {
        _ = client_fut => (),
        _ = dest_fut => ()
    }
}

async fn handle_from_client(
    mut client_receiver: SplitStream<WebSocket>,
    mut dest_sender: SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) {
    while let Some(Ok(msg)) = client_receiver.next().await {
        if dest_sender.send(msg).await.is_err() {
            break;
        }
    }
}

async fn handle_from_dest(
    mut client_sender: SplitSink<WebSocket, Message>,
    mut dest_receiver: SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>,
) {
    while let Some(Ok(msg)) = dest_receiver.next().await {
        if client_sender.send(msg).await.is_err() {
            break;
        }
    }
}
