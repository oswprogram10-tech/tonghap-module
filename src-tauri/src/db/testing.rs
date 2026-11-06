//! 1년치 사용 데이터를 결정적으로 만드는 도우미(성능 점검·수동 점검용).
//! 설계: `docs/design/16-year-scale-performance.md` 1절. 앱 화면 흐름에서는 쓰지 않는다.
//!
//! 무작위는 시드 고정 PRNG(SplitMix64)라서 같은 시드와 설정이면 id·글·날짜가 모두 같다.
//! 검증 규칙을 거치는 `create` 함수 대신 SQL을 한 트랜잭션으로 넣어 빠르게 만든다.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rusqlite::{params, Connection};
use serde_json::Value;

use super::activities;
use super::evaluation::bundle::subject_tags;
use super::evaluation::data::curriculum;
use super::media::{self, MediaKey};
use super::{lock, DbExt, DbResult};

const PHRASES_JSON: &str = include_str!("../../../data/synthetic/phrases.json");

/// 가짜 이름 목록(실제 학생과 무관). 25명을 넘으면 번호를 붙여 돌려 쓴다.
const FAKE_NAMES: [&str; 25] = [
    "가온", "나래", "다솜", "라온", "마루", "바다", "사랑", "아름", "자람", "차오름", "카이", "타리", "파랑", "하늘", "고운", "누리", "도담", "미르", "보람",
    "새봄", "슬기", "여울", "예솔", "이든", "해든",
];

/// 1x1 PNG 모양의 작은 파일(디코딩하지 않는다).
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06,
    0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xFF, 0xFF, 0x3F, 0x00, 0x05, 0xFE, 0x02,
    0xFE, 0xDC, 0xCC, 0x59, 0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[derive(Debug, Clone)]
pub struct SeedOpts {
    pub seed: u64,
    pub students: usize,
    pub observations: usize,
    pub attachments: usize,
    pub units: usize,
    pub sessions: usize,
    pub habits: usize,
    pub habit_days: usize,
    pub parent_reports: usize,
    /// 첫 단원의 차시 수(나머지는 남은 차시를 고르게 나눈다).
    pub big_unit_sessions: usize,
}

impl Default for SeedOpts {
    /// 학급 25명이 1년 쓴 양.
    fn default() -> Self {
        Self {
            seed: 20_250_304,
            students: 25,
            observations: 3_000,
            attachments: 1_500,
            units: 16,
            sessions: 400,
            habits: 4,
            habit_days: 180,
            parent_reports: 50,
            big_unit_sessions: 40,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SeedSummary {
    pub class_id: String,
    pub student_ids: Vec<String>,
    pub unit_ids: Vec<String>,
    /// 차시가 `big_unit_sessions`개인 단원(첫 단원).
    pub big_unit_id: String,
    pub habit_ids: Vec<String>,
    /// 학교 날 첫날·마지막 날.
    pub first_day: String,
    pub last_day: String,
    /// 습관 체크가 있는 마지막 날.
    pub habit_last_day: String,
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `0..n`
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// `lo..=hi`
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn id(&mut self) -> String {
        let v = (u128::from(self.next()) << 64) | u128::from(self.next());
        uuid::Uuid::from_u128(v).to_string()
    }
}

struct Phrases {
    obs: Vec<Vec<String>>,
    quotes: Vec<Vec<String>>,
    session_titles: serde_json::Map<String, Value>,
    habits: Vec<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn load_phrases() -> Phrases {
    let root: Value = serde_json::from_str(PHRASES_JSON).expect("data/synthetic/phrases.json");
    let groups = |key: &str| -> Vec<Vec<String>> {
        root[key].as_object().map(|m| m.values().map(strings).collect()).unwrap_or_default()
    };
    Phrases {
        obs: groups("observations"),
        quotes: groups("quotes"),
        session_titles: root["sessionTitles"].as_object().cloned().unwrap_or_default(),
        habits: strings(&root["habits"]),
    }
}

/// 학교 날(평일) 목록: 3월 둘째 주부터 이듬해 2월까지.
fn school_days(year: i32) -> Vec<NaiveDate> {
    let mut d = NaiveDate::from_ymd_opt(year, 3, 4).expect("날짜");
    let end = NaiveDate::from_ymd_opt(year + 1, 2, 27).expect("날짜");
    let mut out = Vec::new();
    while d <= end {
        if !matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
            out.push(d);
        }
        d += Duration::days(1);
    }
    out
}

fn wav_bytes() -> Vec<u8> {
    let data = 1600u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&16_000u32.to_le_bytes());
    v.extend_from_slice(&32_000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    v.resize(44 + data as usize, 0);
    v
}

/// 메모 20~120자: 같은 갈래 문장을 이어 붙인 뒤 120자에서 자른다.
fn make_note(rng: &mut Rng, ph: &Phrases) -> String {
    let group = &ph.obs[rng.below(ph.obs.len())];
    let target = rng.range(20, 120);
    let mut s = String::new();
    while s.chars().count() < target {
        if !s.is_empty() {
            s.push(' ');
        }
        s += &group[rng.below(group.len())];
    }
    let s: String = s.chars().take(target.max(20)).collect();
    s.trim_end().to_string()
}

fn at(day: NaiveDate, rng: &mut Rng) -> String {
    format!("{}T{:02}:{:02}:00+09:00", day.format("%Y-%m-%d"), rng.range(9, 15), rng.below(60))
}

/// 학급 1, 학생 `opts.students`, 관찰·첨부·단원·차시·습관 체크·평어 초안·학부모 리포트를 넣는다.
/// 첨부 파일은 `media_dir` 아래에 실제로 쓴다.
pub fn seed_year(conn: &mut Connection, media_dir: &Path, key: &MediaKey, opts: &SeedOpts) -> DbResult<SeedSummary> {
    let ph = load_phrases();
    if ph.obs.is_empty() || ph.quotes.is_empty() || ph.habits.is_empty() {
        return Err("합성 문장 자료가 비어 있습니다".to_string());
    }
    let mut rng = Rng(opts.seed);
    let year = 2025;
    let days = school_days(year);
    let ndays = days.len();
    let topics: Vec<(String, i64, i64)> = curriculum().topic_books.iter().map(|t| (t.id.clone(), t.area, t.standard_index)).collect();
    let unit_count = opts.units.min(topics.len()).max(1);
    // 성취기준 코드 풀: 주제책마다 바·슬·즐 하나씩.
    let std_codes = |t: &(String, i64, i64)| -> Vec<String> { ["바", "슬", "즐"].iter().map(|s| format!("[2{s}{:02}-{:02}]", t.1, t.2)).collect() };
    let mut tag_pool: Vec<&str> = ["바", "슬", "즐"].iter().flat_map(|s| subject_tags(s)).collect::<HashSet<_>>().into_iter().collect();
    tag_pool.sort_unstable();
    if tag_pool.is_empty() {
        tag_pool = vec!["협력", "탐구"];
    }
    let now = format!("{year}-03-04T09:00:00+09:00");

    let tx = conn.transaction().db()?;
    let class_id = rng.id();
    tx.execute(
        "INSERT INTO classes (id, school_year, grade, name, created_at) VALUES (?1, ?2, 1, '1년치 점검 학급', ?3)",
        params![class_id, year, now],
    )
    .db()?;
    let mut student_ids = Vec::new();
    for n in 0..opts.students {
        let id = rng.id();
        let name = if n < FAKE_NAMES.len() { FAKE_NAMES[n].to_string() } else { format!("{}{}", FAKE_NAMES[n % FAKE_NAMES.len()], n / FAKE_NAMES.len() + 1) };
        tx.execute("INSERT INTO students (id, class_id, number, name, active, created_at) VALUES (?1, ?2, ?3, ?4, 1, ?5)", params![id, class_id, n as i64 + 1, name, now]).db()?;
        student_ids.push(id);
    }

    // 단원·단계·차시. 차시는 단원 순서대로 학교 날에 퍼 놓는다.
    let mut unit_ids = Vec::new();
    let mut sessions_by_unit: Vec<Vec<String>> = Vec::new();
    let big = opts.big_unit_sessions.min(opts.sessions);
    let rest_units = unit_count.saturating_sub(1).max(1);
    let rest_sessions = opts.sessions - big;
    let stage_titles = ["만나기", "탐구하기", "표현하기", "나누기"];
    let builtin = activities::builtin();
    let mut global = 0usize;
    for (u, topic) in topics.iter().take(unit_count).enumerate() {
        let n_sessions = if u == 0 {
            if unit_count == 1 { opts.sessions } else { big }
        } else {
            rest_sessions / rest_units + usize::from(u - 1 < rest_sessions % rest_units)
        };
        let uid = rng.id();
        let title = curriculum().topic_books.iter().find(|t| t.id == topic.0).map(|t| t.title.clone()).unwrap_or_default();
        tx.execute(
            "INSERT INTO units (id, class_id, topic_book_id, tool_id, title, memo, created_at, updated_at) VALUES (?1, ?2, ?3, 'standards-first', ?4, '', ?5, ?5)",
            params![uid, class_id, topic.0, format!("{title} 단원"), now],
        )
        .db()?;
        let mut stage_ids = Vec::new();
        for (p, st) in stage_titles.iter().enumerate() {
            let sid = rng.id();
            tx.execute("INSERT INTO unit_stages (id, unit_id, position, title) VALUES (?1, ?2, ?3, ?4)", params![sid, uid, p as i64, st]).db()?;
            stage_ids.push(sid);
        }
        let titles = strings(ph.session_titles.get(&topic.0).unwrap_or(&Value::Null));
        let cards: Vec<&activities::Activity> = builtin.iter().filter(|a| a.topic_book_id == topic.0).collect();
        let codes = std_codes(topic);
        let mut stage_count = [0i64; 4];
        let mut ids: Vec<String> = Vec::new();
        for i in 0..n_sessions {
            let sid = rng.id();
            let stage = (i * stage_titles.len() / n_sessions.max(1)).min(3);
            let pos = stage_count[stage];
            stage_count[stage] += 1;
            let kind = match i % 10 {
                3 => "safety",
                7 => "makingbook",
                5 => "play",
                _ => "general",
            };
            let day = days[(global * ndays / opts.sessions.max(1)).min(ndays - 1)];
            global += 1;
            let status = if i * 10 < n_sessions * 7 { "done" } else { "planned" };
            let follows = if i % 4 == 3 { ids.last().cloned() } else { None };
            let activity_id = if !cards.is_empty() && rng.chance(30) { Some(cards[rng.below(cards.len())].id.clone()) } else { None };
            let stitle = if titles.is_empty() { format!("{}차시", i + 1) } else { format!("{} {}", titles[i % titles.len()], i / titles.len() + 1) };
            let questions = if rng.chance(30) { ph.quotes[rng.below(ph.quotes.len())][0].clone() } else { String::new() };
            tx.execute(
                "INSERT INTO unit_sessions (id, unit_id, stage_id, position, title, plan, minutes, kind, scheduled_on, status,
                                            student_questions, reflection, follows_session_id, created_at, updated_at, activity_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, '', 40, ?6, ?7, ?8, ?9, '', ?10, ?11, ?11, ?12)",
                params![sid, uid, stage_ids[stage], pos, stitle, kind, day.format("%Y-%m-%d").to_string(), status, questions, follows, now, activity_id],
            )
            .db()?;
            for (k, code) in codes.iter().enumerate().take(rng.range(0, 2)) {
                let mode = ["repeat", "focus", "emergent"][k % 3];
                tx.execute("INSERT INTO unit_session_standards (session_id, standard_code, mode) VALUES (?1, ?2, ?3)", params![sid, code, mode]).db()?;
            }
            ids.push(sid);
        }
        unit_ids.push(uid);
        sessions_by_unit.push(ids);
    }

    // 관찰: 날짜는 오름차순으로 퍼 놓고, 주제책은 차례대로 고르게 나눈다.
    let mut obs_ids = Vec::with_capacity(opts.observations);
    {
        let mut ins = tx
            .prepare(
                "INSERT INTO observations (id, class_id, observed_at, topic_book_id, note, created_at, updated_at, unit_session_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?3, ?3, ?6)",
            )
            .db()?;
        let mut ins_stu = tx.prepare("INSERT INTO observation_students (observation_id, student_id) VALUES (?1, ?2)").db()?;
        let mut ins_std = tx.prepare("INSERT INTO observation_standards (observation_id, standard_code) VALUES (?1, ?2)").db()?;
        let mut ins_tag = tx.prepare("INSERT INTO observation_tags (observation_id, tag) VALUES (?1, ?2)").db()?;
        for i in 0..opts.observations {
            let id = rng.id();
            let day = days[(i * ndays / opts.observations.max(1)).min(ndays - 1)];
            let ti = (i * unit_count / opts.observations.max(1)).min(unit_count - 1);
            let topic = &topics[ti];
            let session = if rng.chance(40) { Some(sessions_by_unit[ti][rng.below(sessions_by_unit[ti].len())].clone()) } else { None };
            let note = make_note(&mut rng, &ph);
            ins.execute(params![id, class_id, at(day, &mut rng), topic.0, note, session]).db()?;
            let mut chosen: HashSet<usize> = HashSet::new();
            let want = rng.range(1, 4).min(student_ids.len());
            while chosen.len() < want {
                chosen.insert(rng.below(student_ids.len()));
            }
            let mut chosen: Vec<usize> = chosen.into_iter().collect();
            chosen.sort_unstable();
            for s in chosen {
                ins_stu.execute(params![id, student_ids[s]]).db()?;
            }
            let codes = std_codes(topic);
            for code in codes.iter().take(rng.range(0, 2)) {
                ins_std.execute(params![id, code]).db()?;
            }
            let mut tags: Vec<&str> = Vec::new();
            while tags.len() < rng.range(1, 3).min(tag_pool.len()) {
                let t = tag_pool[rng.below(tag_pool.len())];
                if !tags.contains(&t) {
                    tags.push(t);
                }
            }
            for t in tags {
                ins_tag.execute(params![id, t]).db()?;
            }
            obs_ids.push(id);
        }
    }

    // 첨부: 사진 60%·음성 40%. 파일을 실제로 쓴다(크기보다 개수가 중요).
    if !obs_ids.is_empty() {
        let wav = wav_bytes();
        let mut made_dirs: HashSet<String> = HashSet::new();
        let mut ins = tx
            .prepare(
                "INSERT INTO attachments (id, observation_id, kind, rel_path, mime, bytes, transcript, created_at, transcript_status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .db()?;
        for i in 0..opts.attachments {
            let id = rng.id();
            let oi = rng.below(obs_ids.len());
            let ym = format!("{year}/{:02}", days[(oi * ndays / obs_ids.len()).min(ndays - 1)].month());
            if made_dirs.insert(ym.clone()) {
                std::fs::create_dir_all(media_dir.join(&ym)).map_err(|e| format!("첨부 폴더를 만들 수 없습니다: {e}"))?;
            }
            let photo = i % 5 < 3;
            let (kind, ext, mime, data): (&str, &str, &str, &[u8]) = if photo { ("photo", "png", "image/png", TINY_PNG) } else { ("audio", "wav", "audio/wav", &wav) };
            let rel = format!("{ym}/{id}.{ext}");
            media::write_encrypted(key, &media_dir.join(&rel), &rel, data).map_err(|e| format!("첨부 파일을 쓸 수 없습니다: {e}"))?;
            let (status, transcript): (&str, Option<String>) = if photo {
                ("none", None)
            } else if rng.chance(70) {
                let g = &ph.quotes[rng.below(ph.quotes.len())];
                ("done", Some(g[rng.below(g.len())].clone()))
            } else if rng.chance(66) {
                ("none", None)
            } else {
                ("failed", None)
            };
            ins.execute(params![id, obs_ids[oi], kind, rel, mime, data.len() as i64, transcript, now, status]).db()?;
        }
    }

    // 습관 체크: 첫 `habit_days` 학교 날 × 학생.
    let mut habit_ids = Vec::new();
    let check_days = &days[..opts.habit_days.min(ndays)];
    {
        let mut ins = tx.prepare("INSERT INTO habit_checks (habit_id, student_id, checked_on, done, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)").db()?;
        for h in 0..opts.habits {
            let hid = rng.id();
            tx.execute(
                "INSERT INTO habits (id, class_id, title, icon, active, position, created_at) VALUES (?1, ?2, ?3, '', 1, ?4, ?5)",
                params![hid, class_id, ph.habits[h % ph.habits.len()], h as i64, now],
            )
            .db()?;
            for d in check_days {
                let ds = d.format("%Y-%m-%d").to_string();
                for sid in &student_ids {
                    ins.execute(params![hid, sid, ds, i64::from(rng.chance(80)), now]).db()?;
                }
            }
            habit_ids.push(hid);
        }
    }

    // 평어 초안(학생 × 교과 일부)과 학부모 리포트.
    let (first_day, last_day) = (days[0].format("%Y-%m-%d").to_string(), days[ndays - 1].format("%Y-%m-%d").to_string());
    for (si, sid) in student_ids.iter().enumerate() {
        for (k, subject) in ["바", "슬", "즐"].iter().enumerate() {
            if (si + k) % 3 == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO evaluation_drafts (id, class_id, student_id, subject, period_from, period_to, sentences, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '[]', ?7, ?7)",
                params![rng.id(), class_id, sid, subject, first_day, last_day, now],
            )
            .db()?;
        }
    }
    for i in 0..opts.parent_reports {
        let sid = &student_ids[i % student_ids.len()];
        let topic = &topics[(i / student_ids.len()) % unit_count].0;
        tx.execute(
            "INSERT INTO parent_reports (id, class_id, student_id, topic_book_id, period_from, period_to, message, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, ?7)",
            params![rng.id(), class_id, sid, topic, first_day, last_day, now],
        )
        .db()?;
    }
    tx.commit().db()?;

    Ok(SeedSummary {
        class_id,
        student_ids,
        big_unit_id: unit_ids[0].clone(),
        unit_ids,
        habit_ids,
        first_day,
        last_day,
        habit_last_day: check_days.last().map(|d| d.format("%Y-%m-%d").to_string()).unwrap_or_default(),
    })
}

// ───────── 디버그 전용 명령 ─────────

/// E2E·수동 점검용: 열려 있는 DB에 1년치 데이터를 더한다. 릴리스 빌드에서는 항상 거부한다.
#[tauri::command]
pub async fn dev_seed_year(app: tauri::AppHandle) -> DbResult<String> {
    if !cfg!(debug_assertions) {
        return Err("개발용 명령입니다".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let media = app.state::<super::MediaDir>().0.clone();
        let db = app.state::<Mutex<Connection>>();
        let mut conn = lock(&db)?;
        let key = app.state::<media::MediaKeyHolder>().require()?;
        let s = seed_year(&mut conn, &media, &key, &SeedOpts::default())?;
        Ok(s.class_id)
    })
    .await
    .map_err(|e| format!("작업이 중단되었습니다: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small() -> SeedOpts {
        SeedOpts { observations: 120, attachments: 40, sessions: 48, big_unit_sessions: 8, habit_days: 10, parent_reports: 5, ..SeedOpts::default() }
    }

    fn fresh() -> Connection {
        crate::db::test_conn()
    }

    fn dump(conn: &Connection) -> Vec<String> {
        let q = |sql: &str| -> Vec<String> {
            let mut stmt = conn.prepare(sql).unwrap();
            let n = stmt.column_count();
            stmt.query_map([], |r| Ok((0..n).map(|i| format!("{:?}", r.get::<_, rusqlite::types::Value>(i).unwrap())).collect::<Vec<_>>().join("|")))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let mut all = q("SELECT * FROM observations ORDER BY id");
        all.extend(q("SELECT * FROM unit_sessions ORDER BY id"));
        all.extend(q("SELECT * FROM attachments ORDER BY id"));
        all.extend(q("SELECT * FROM observation_students ORDER BY 1, 2"));
        all
    }

    #[test]
    fn same_seed_gives_identical_data_and_other_seed_differs() {
        let (d1, d2, d3) = (tempfile_dir("a"), tempfile_dir("b"), tempfile_dir("c"));
        let (mut a, mut b, mut c) = (fresh(), fresh(), fresh());
        seed_year(&mut a, &d1, &crate::db::media::test_key(), &small()).unwrap();
        seed_year(&mut b, &d2, &crate::db::media::test_key(), &small()).unwrap();
        seed_year(&mut c, &d3, &crate::db::media::test_key(), &SeedOpts { seed: 7, ..small() }).unwrap();
        assert_eq!(dump(&a), dump(&b));
        assert_ne!(dump(&a), dump(&c));
    }

    fn tempfile_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tonghap-seed-{}-{tag}-{}", std::process::id(), crate::db::new_id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn counts_are_exact_and_foreign_keys_hold() {
        let dir = tempfile_dir("counts");
        let mut conn = fresh();
        let o = small();
        let s = seed_year(&mut conn, &dir, &crate::db::media::test_key(), &o).unwrap();
        assert_eq!(count(&conn, "SELECT count(*) FROM classes"), 1);
        assert_eq!(count(&conn, "SELECT count(*) FROM students"), 25);
        assert_eq!(count(&conn, "SELECT count(*) FROM observations"), 120);
        assert_eq!(count(&conn, "SELECT count(*) FROM attachments"), 40);
        assert_eq!(count(&conn, "SELECT count(*) FROM units"), 16);
        assert_eq!(count(&conn, "SELECT count(*) FROM unit_sessions"), 48);
        assert_eq!(count(&conn, &format!("SELECT count(*) FROM unit_sessions WHERE unit_id = '{}'", s.big_unit_id)), 8);
        assert_eq!(count(&conn, "SELECT count(*) FROM habits"), 4);
        assert_eq!(count(&conn, "SELECT count(*) FROM habit_checks"), 4 * 25 * 10);
        assert_eq!(count(&conn, "SELECT count(*) FROM parent_reports"), 5);
        assert!(count(&conn, "SELECT count(*) FROM evaluation_drafts") > 0);
        // 관찰마다 학생 1~4명, 태그 1~3, 성취기준 0~2, 메모 20~120자
        assert_eq!(count(&conn, "SELECT count(*) FROM observations WHERE length(note) NOT BETWEEN 20 AND 120"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM (SELECT observation_id, count(*) n FROM observation_students GROUP BY 1) WHERE n NOT BETWEEN 1 AND 4"), 0);
        assert_eq!(count(&conn, "SELECT count(*) FROM observations WHERE id NOT IN (SELECT observation_id FROM observation_tags)"), 0);
        // 첨부 파일이 실제로 있다
        let rel: String = conn.query_row("SELECT rel_path FROM attachments LIMIT 1", [], |r| r.get(0)).unwrap();
        assert!(dir.join(rel).is_file());
        // 주제책 16권에 고르게
        assert_eq!(count(&conn, "SELECT count(DISTINCT topic_book_id) FROM observations"), 16);
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let bad: Vec<String> = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert!(bad.is_empty(), "외래 키 위반: {bad:?}");
        drop(stmt);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_year_has_the_designed_volume() {
        let dir = tempfile_dir("full");
        let mut conn = fresh();
        let s = seed_year(&mut conn, &dir, &crate::db::media::test_key(), &SeedOpts::default()).unwrap();
        assert_eq!(count(&conn, "SELECT count(*) FROM observations"), 3000);
        assert_eq!(count(&conn, "SELECT count(*) FROM attachments"), 1500);
        assert_eq!(count(&conn, "SELECT count(*) FROM unit_sessions"), 400);
        assert_eq!(count(&conn, "SELECT count(*) FROM habit_checks"), 4 * 25 * 180);
        assert_eq!(count(&conn, &format!("SELECT count(*) FROM unit_sessions WHERE unit_id = '{}'", s.big_unit_id)), 40);
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        assert!(stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().next().is_none());
        drop(stmt);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
