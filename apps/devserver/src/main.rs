//! 開發用 HTTP 外殼。
//!
//! **這不是產品的一部分。** M3 會由 Tauri command 取代它，command 只是薄殼，
//! 呼叫的是同一組 [`poker_ipc::IpcHandler`] 方法，因此前端換過去時不必改動
//! 呼叫形狀（路徑對應 command 名稱、query 對應參數）。
//!
//! 之所以先做這一層，是因為 Tauri 在 Linux 需要 webkit2gtk，而開發機沒有；
//! 引擎與 IPC 契約本身不依賴 GUI，先用 HTTP 打通垂直切片可以讓前端立即開工。
//!
//! 用 std 手寫最小 HTTP/1.1 而不引入 HTTP 框架：這只是開發鷹架，
//! 不值得為它增加會進入 Cargo.lock 的相依。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use poker_engine::betting::Action;
use poker_engine::chips::Chips;
use poker_engine::hand::ActionProvider;
use poker_engine::strategy::DecisionView;
use poker_engine::rng::RNG_VERSION;
use poker_engine::session::{run_session, SessionConfig};
use poker_engine::table::TableConfig;
use poker_ipc::{HoleCardVisibility, IpcHandler};
use poker_storage::codec::{HandRecord, LOG_FORMAT_VERSION};
use poker_storage::manifest::{
    ContentSnapshot, ExecutionMode, InstanceRecord, RuleVariants, RunManifest, SCHEMA_VERSION,
};
use poker_storage::Store;

const ADDR: &str = "127.0.0.1:7801";
const HERO_SEAT: usize = 0;

/// 示範用的行動來源：能過牌就過牌，否則跟注。
///
/// M2 會由使用者策略與 Bot 決策取代。
struct CallingStation;

impl ActionProvider for CallingStation {
    fn choose(&mut self, view: &DecisionView) -> Action {
        let legal = &view.legal;
        if legal.can_check {
            Action::Check
        } else if legal.call_to.is_some() {
            Action::Call
        } else {
            Action::AllIn
        }
    }
}

fn seed_run() -> (IpcHandler, i64) {
    let config = SessionConfig {
        table: TableConfig::simple(1, 2),
        players: 9,
        starting_stacks: vec![Chips::new(400); 9],
        auto_refill: Some(9),
        hero_seat: HERO_SEAT,
        hand_limit: 500,
        master_seed: 20_260_816,
    };

    let mut store = Store::open_in_memory().expect("建立記憶體資料庫");
    let manifest = build_manifest(&config);
    let run_id = store.create_run(&manifest).expect("建立 run");

    let mut rows = Vec::new();
    let summary = run_session(&config, &mut CallingStation, |played| {
        let record = HandRecord::from_played(played);
        let contributed = played.result.total_contributions[HERO_SEAT];
        let delta = record.hero_delta(HERO_SEAT, contributed);
        rows.push((record, played.seated, delta));
    });
    for chunk in rows.chunks(500) {
        store.write_hands(run_id, chunk).expect("寫入 log");
    }

    let mut final_manifest = build_manifest(&config);
    final_manifest.instances = summary
        .instances
        .iter()
        .map(|i| InstanceRecord {
            index: i.index,
            first_hand: 0,
            last_hand: i.hands.saturating_sub(1),
            hands: i.hands,
            end: format!("{:?}", i.end),
            refills: Vec::new(),
        })
        .collect();
    final_manifest.completed = true;
    store
        .finish_run(run_id, &final_manifest, summary.hands_played)
        .expect("結束 run");

    println!(
        "已產生示範 run：{} 手、{} 個桌次",
        summary.hands_played,
        summary.instances.len()
    );
    (IpcHandler::new(store), run_id)
}

fn build_manifest(config: &SessionConfig) -> RunManifest {
    RunManifest {
        engine_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: SCHEMA_VERSION,
        log_format_version: LOG_FORMAT_VERSION,
        rng_algorithm: RNG_VERSION.to_owned(),
        stream_derivation: "splitmix64(master_seed, hand_index, domain) → xoshiro256**".to_owned(),
        master_seed: config.master_seed,
        execution_mode: ExecutionMode::Batch,
        hand_limit: config.hand_limit,
        players: config.players,
        hero_seat: config.hero_seat,
        starting_stacks: config.starting_stacks.iter().map(|c| c.units()).collect(),
        small_blind: config.table.small_blind.units(),
        big_blind: config.table.big_blind.units(),
        ante_mode: "none".to_owned(),
        ante_amount: 0,
        straddle_amounts: Vec::new(),
        rake_basis_points: 0,
        rake_cap: 0,
        rake_no_flop_no_drop: false,
        stack_policy: "bustOut".to_owned(),
        auto_refill_target: config.auto_refill,
        rule_variants: RuleVariants::default(),
        hero_strategy: ContentSnapshot::new(
            "示範策略",
            "v0",
            serde_json::json!({ "note": "M2 前的佔位內容" }),
        ),
        bot_personas: Vec::new(),
        baseline_version: "none".to_owned(),
        instances: Vec::new(),
        created_at: 1_771_200_000,
        completed: false,
        checkpoint_version: 1,
    }
}

fn main() {
    let (handler, run_id) = seed_run();
    let listener = TcpListener::bind(ADDR).expect("綁定連接埠");
    println!("dev server 已啟動：http://{ADDR}（run_id={run_id}）");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = serve(stream, &handler, run_id) {
                    eprintln!("處理請求失敗：{error}");
                }
            }
            Err(error) => eprintln!("連線失敗：{error}"),
        }
    }
}

fn serve(mut stream: TcpStream, handler: &IpcHandler, run_id: i64) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    let target = request_line.split_whitespace().nth(1).unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let body = match path {
        "/api/run" => handler
            .get_run(run_id)
            .ok()
            .and_then(|view| serde_json::to_string(&view).ok()),
        "/api/hands" => {
            let offset = param(query, "offset").unwrap_or(0);
            let limit = param(query, "limit").unwrap_or(50).min(500);
            handler
                .list_hands(run_id, offset, limit)
                .ok()
                .and_then(|view| serde_json::to_string(&view).ok())
        }
        "/api/hand" => {
            let index = param(query, "index").unwrap_or(0);
            // 預設只送出依現實規則亮出的底牌；?revealAll=1 才全揭露，
            // 對應核心規格 2.4「重播是否顯示未攤牌底牌採明確設定，預設不顯示」
            let visibility = if param(query, "revealAll") == Some(1) {
                HoleCardVisibility::All
            } else {
                HoleCardVisibility::RevealedOnly
            };
            handler
                .get_hand(run_id, index, visibility)
                .ok()
                .and_then(|view| serde_json::to_string(&view).ok())
        }
        _ => None,
    };

    let response = match body {
        Some(json) => format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json; charset=utf-8\r\n\
             Access-Control-Allow-Origin: *\r\n\
             Content-Length: {}\r\n\r\n{json}",
            json.len()
        ),
        None => "HTTP/1.1 404 Not Found\r\n\
                 Access-Control-Allow-Origin: *\r\n\
                 Content-Length: 0\r\n\r\n"
            .to_owned(),
    };

    stream.write_all(response.as_bytes())?;
    stream.flush()
}

fn param(query: &str, key: &str) -> Option<u64> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .and_then(|(_, value)| value.parse().ok())
}
