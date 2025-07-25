use monoio::io::{sink::SinkExt, stream::Stream};
use monoio_tungstenite::{Error, Result, connect};

const AGENT: &str = "Tungstenite";

async fn get_case_count() -> Result<u32> {
    let (mut socket, _) = connect("ws://127.0.0.1:9001/getCaseCount").await?;
    let msg = socket.next().await.expect("Can't fetch case count")?;
    socket.close(None).await?;
    Ok(msg
        .to_text()?
        .parse::<u32>()
        .expect("Can't parse case count"))
}

async fn update_reports() -> Result<()> {
    let (mut socket, _) =
        connect(&format!("ws://127.0.0.1:9001/updateReports?agent={AGENT}")).await?;
    socket.close(None).await?;
    Ok(())
}

async fn run_test(case: u32) -> Result<()> {
    let case_url = &format!("ws://127.0.0.1:9001/runCase?case={case}&agent={AGENT}");
    let (mut ws_stream, _) = connect(case_url).await?;

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if msg.is_text() || msg.is_binary() {
            ws_stream.send_and_flush(msg).await?;
        }
    }

    Ok(())
}

#[monoio::main]
async fn main() {
    let total = get_case_count().await.expect("Error getting case count");

    for case in 1..=total {
        if let Err(e) = run_test(case).await {
            match e {
                Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8(_) => (),
                err => eprintln!("Testcase {case} failed with error: {err}"),
            }
        }
    }

    update_reports().await.expect("Error updating reports");
}
