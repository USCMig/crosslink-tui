//! Layout and actions: a title bar, a menu on the left, and one panel at a time on the right.

use crate::config::{self, ConfigFile};
use crate::node;
use crate::poll::Snap;
use crate::render;
use crate::rpc::Rpc;
use cursive::event::Key;
use cursive::theme::{BaseColor, BorderStyle, Color, ColorStyle, PaletteColor, Theme};
use cursive::traits::*;
use cursive::utils::markup::StyledString;
use cursive::view::ScrollStrategy;
use cursive::views::{
    DialogFocus,
    Button, Checkbox, Dialog, DummyView, EditView, Layer, LinearLayout, ListView, Panel, ScrollView, SelectView, StackView,
    TextArea, TextView,
};
use cursive::Cursive;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A command to run with the terminal handed back to the user (sudo password, $EDITOR).
pub struct Pending {
    pub cmd: Vec<String>,
    pub reload_config: bool,
}

pub struct App {
    pub snap: Arc<Mutex<Snap>>,
    pub cfg: Arc<Mutex<ConfigFile>>,
    pub rpc: Rpc,
    pub service: String,
    pub argv: Vec<String>,
    pub log_desc: String,
    pub pending: Arc<Mutex<Option<Pending>>>,
    pub want_logs: Arc<AtomicBool>,
    pub panel: Arc<Mutex<String>>,
    pub log_filter: usize,
    pub hide_noise: bool,
    /// Started with --read-only: nothing that changes the node, its wallet or its config runs.
    pub read_only: bool,
    last_roster: Vec<String>,
    last_bonds: Vec<String>,
}

impl App {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        snap: Arc<Mutex<Snap>>,
        cfg: Arc<Mutex<ConfigFile>>,
        rpc: Rpc,
        service: String,
        argv: Vec<String>,
        log_desc: String,
        pending: Arc<Mutex<Option<Pending>>>,
        want_logs: Arc<AtomicBool>,
        panel: Arc<Mutex<String>>,
        read_only: bool,
    ) -> Self {
        Self {
            snap, cfg, rpc, service, argv, log_desc, pending, want_logs, panel,
            log_filter: 0, hide_noise: true, read_only, last_roster: Vec::new(), last_bonds: Vec::new(),
        }
    }
}

const PANELS: [(&str, &str); 10] = [
    ("basic", "Basic Status"),
    ("peers", "Peers and Sync"),
    ("mining", "Mining"),
    ("staking", "Staking"),
    ("stakers", "Top Stakers"),
    ("bft", "BFT Finality"),
    ("config", "Config"),
    ("control", "Node Control"),
    ("logs", "Logs"),
    ("version", "Version Info"),
];

/// Fee headroom kept back by "Stake all available".
const STAKE_FEE_BUFFER_ZATS: u64 = 5_000_000;

fn app(s: &mut Cursive) -> &mut App {
    s.user_data::<App>().expect("App is installed before the UI runs")
}

fn heading(text: &str) -> TextView {
    TextView::new(StyledString::styled(text, ColorStyle::title_primary()))
}

fn set_text(s: &mut Cursive, name: &str, text: impl Into<StyledString>) {
    let text = text.into();
    s.call_on_name(name, move |v: &mut TextView| v.set_content(text));
}

fn info(s: &mut Cursive, title: &str, text: impl Into<String>) {
    s.add_layer(Dialog::info(text.into()).title(title));
}

/// True (after telling the user why) when the TUI runs read-only. Every action that writes to the
/// node, its wallet or its config calls this first, even when its button is already hidden.
fn blocked(s: &mut Cursive) -> bool {
    if !app(s).read_only {
        return false;
    }
    info(s, "Read-only mode", "This TUI was started with --read-only, so it does not change the node, its wallet or its config.");
    true
}

const READ_ONLY_NOTE: &str = "Read-only mode (--read-only): actions are turned off on this panel.";

fn edit_content(s: &mut Cursive, name: &str) -> String {
    s.call_on_name(name, |v: &mut EditView| v.get_content().to_string()).unwrap_or_default()
}

fn checked(s: &mut Cursive, name: &str) -> bool {
    s.call_on_name(name, |v: &mut Checkbox| v.is_checked()).unwrap_or(false)
}

fn apply_theme(siv: &mut Cursive) {
    let mut t = Theme::terminal_default();
    t.shadow = false;
    t.borders = BorderStyle::Simple;
    let p = &mut t.palette;
    p[PaletteColor::Background] = Color::TerminalDefault;
    p[PaletteColor::View] = Color::TerminalDefault;
    p[PaletteColor::Shadow] = Color::TerminalDefault;
    p[PaletteColor::Primary] = Color::TerminalDefault;
    p[PaletteColor::Secondary] = Color::Light(BaseColor::Black);
    p[PaletteColor::Tertiary] = Color::Light(BaseColor::White);
    p[PaletteColor::TitlePrimary] = Color::Light(BaseColor::Green);
    p[PaletteColor::TitleSecondary] = Color::Light(BaseColor::Yellow);
    p[PaletteColor::Highlight] = Color::Rgb(26, 188, 156);
    p[PaletteColor::HighlightInactive] = Color::Rgb(14, 110, 92);
    p[PaletteColor::HighlightText] = Color::Dark(BaseColor::Black);
    siv.set_theme(t);
}

pub fn build(siv: &mut Cursive, app_state: App) {
    apply_theme(siv);
    let start = app_state.panel.lock().unwrap().clone();
    let cfg = app_state.cfg.clone();
    let ro = app_state.read_only;
    siv.set_user_data(app_state);

    let mut menu = SelectView::<&'static str>::new();
    for (id, label) in PANELS {
        menu.add_item(label, id);
    }
    menu.set_on_select(|s, id: &&'static str| show_panel(s, id));
    menu.set_on_submit(|s, _: &&'static str| focus_column(s, 1));
    let start_index = PANELS.iter().position(|(id, _)| *id == start).unwrap_or(0);
    menu.set_selection(start_index);

    let help = TextView::new("--------------------\nTab/Arrow : Cycle\nEnter     : Select\nEsc       : Back\nQ         : Quit");
    let left = Panel::new(
        LinearLayout::vertical()
            .child(menu.with_name("menu"))
            .child(DummyView.full_height())
            .child(help),
    )
    .fixed_width(24);

    let mut stack = StackView::new();
    {
        let cfg = cfg.lock().unwrap();
        stack.add_fullscreen_layer(Layer::new(text_panel("version_text").full_screen()).with_name("version_panel"));
        stack.add_fullscreen_layer(Layer::new(logs_panel().full_screen()).with_name("logs_panel"));
        stack.add_fullscreen_layer(Layer::new(control_panel(ro).full_screen()).with_name("control_panel"));
        stack.add_fullscreen_layer(Layer::new(config_panel(ro).full_screen()).with_name("config_panel"));
        stack.add_fullscreen_layer(Layer::new(text_panel("bft_text").full_screen()).with_name("bft_panel"));
        stack.add_fullscreen_layer(Layer::new(text_panel("stakers_text").full_screen()).with_name("stakers_panel"));
        stack.add_fullscreen_layer(Layer::new(staking_panel(ro).full_screen()).with_name("staking_panel"));
        stack.add_fullscreen_layer(Layer::new(mining_panel(&cfg, ro).full_screen()).with_name("mining_panel"));
        stack.add_fullscreen_layer(Layer::new(text_panel("peers_text").full_screen()).with_name("peers_panel"));
        stack.add_fullscreen_layer(Layer::new(text_panel("basic_text").full_screen()).with_name("basic_panel"));
    }

    let root = LinearLayout::vertical()
        .child(Panel::new(TextView::new("").with_name("header")))
        .child(
            LinearLayout::horizontal()
                .child(left)
                .child(Panel::new(stack.with_name("stack")).full_screen())
                .with_name("columns"),
        );
    siv.add_fullscreen_layer(root);

    siv.add_global_callback('q', confirm_quit);
    siv.add_global_callback('Q', confirm_quit);
    siv.add_global_callback(Key::Esc, |s| {
        if s.screen().len() > 1 {
            s.pop_layer();
        } else {
            focus_column(s, 0);
        }
    });

    refresh_config_list(siv);
    show_panel(siv, PANELS[start_index].0);
}

/// 0 = menu, 1 = panel. Moving the column focus (rather than focusing a view by name) lets the
/// panel hand focus to its first control.
fn focus_column(s: &mut Cursive, i: usize) {
    s.call_on_name("columns", |l: &mut LinearLayout| {
        let _ = l.set_focus_index(i);
    });
}

fn show_panel(s: &mut Cursive, id: &str) {
    let layer = format!("{id}_panel");
    s.call_on_name("stack", |st: &mut StackView| {
        if let Some(pos) = st.find_layer_from_name(&layer) {
            st.move_to_front(pos);
        }
    });
    let a = app(s);
    *a.panel.lock().unwrap() = id.to_string();
    a.want_logs.store(id == "logs", Ordering::Relaxed);
    refresh(s);
}

fn text_panel(name: &str) -> impl View {
    ScrollView::new(TextView::new("Loading...").with_name(name)).full_screen()
}

fn confirm_quit(s: &mut Cursive) {
    if s.screen().len() > 1 {
        return;
    }
    if app(s).cfg.lock().unwrap().dirty {
        s.add_layer(
            Dialog::text("The config has unsaved changes. Quit anyway?")
                .title("Quit")
                .button("Quit", |s| s.quit())
                .dismiss_button("Cancel"),
        );
    } else {
        s.quit();
    }
}

/// Re-renders every panel from the latest snapshot. Called by the poller and after actions.
pub fn refresh(s: &mut Cursive) {
    let a = app(s);
    let snap = a.snap.lock().unwrap().clone();
    let (service, rpc_url, argv, filter, hide, log_desc) =
        (a.service.clone(), a.rpc.url.clone(), a.argv.clone(), a.log_filter, a.hide_noise, a.log_desc.clone());
    let (config_path, threads, dirty) = {
        let c = a.cfg.lock().unwrap();
        (c.path.display().to_string(), c.get_int(&["mining", "internal_miner_threads"]), c.dirty)
    };
    let panel = a.panel.lock().unwrap().clone();
    let read_only = a.read_only;

    let build = snap.info.get("build").and_then(Value::as_str).unwrap_or("?").to_string();
    let mut header = StyledString::styled(format!("Crosslink Node {build} [Crosslink Feature Net]"), ColorStyle::title_primary());
    header.append_plain(format!("    {}    RPC {rpc_url}", render::status_word(&snap)));
    if dirty {
        header.append_styled("    config: unsaved changes", ColorStyle::title_secondary());
    }
    if read_only {
        header.append_styled("    READ-ONLY", ColorStyle::title_secondary());
    }
    if snap.rpc_error.is_some() {
        header.append_styled("    NOT LIVE", ColorStyle::title_secondary());
    }
    set_text(s, "header", header);

    // Only the visible panel is re-rendered; switching panels triggers a refresh.
    match panel.as_str() {
        "basic" => set_text(s, "basic_text", render::with_stale_note(&snap, render::basic(&snap))),
        "peers" => set_text(s, "peers_text", render::with_stale_note(&snap, render::peers(&snap))),
        "mining" => {
            let env = snap.service.get("Environment").cloned();
            set_text(s, "mining_text", render::with_stale_note(&snap, render::mining_status(&snap, threads, env.as_deref())));
        }
        "staking" => {
            set_text(s, "staking_text", render::with_stale_note(&snap, render::staking(&snap)));
            refresh_roster(s, &snap);
            refresh_bonds(s, &snap);
        }
        "stakers" => set_text(s, "stakers_text", render::top_stakers(&snap)),
        "bft" => set_text(s, "bft_text", render::with_stale_note(&snap, render::bft(&snap))),
        "config" => set_text(s, "config_status", config_status(&config_path, dirty)),
        "control" => set_text(s, "control_text", render::node_control(&snap, &service)),
        "logs" => set_text(s, "logs_text", render::logs(&snap, filter, hide)),
        "version" => set_text(s, "version_text", render::version(&snap, &service, &config_path, &rpc_url, &argv, &log_desc)),
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Mining

fn mining_panel(cfg: &ConfigFile, read_only: bool) -> impl View {
    if read_only {
        let (enabled, addr, threads, low) = mining_values(cfg);
        let settings = format!(
            "Internal miner enabled:  {enabled}\nPayout address:          {addr}\nMining threads:          {threads}\nLow CPU priority:        {low}"
        );
        return LinearLayout::vertical()
            .child(TextView::new("Loading...").with_name("mining_text"))
            .child(DummyView)
            .child(heading("Miner settings ([mining] in the config file, as loaded)"))
            .child(TextView::new(settings))
            .child(DummyView)
            .child(TextView::new(READ_ONLY_NOTE).style(ColorStyle::secondary()))
            .scrollable();
    }
    let form = ListView::new()
        .child("Internal miner enabled", Checkbox::new().with_name("m_enabled"))
        .child("Payout address", EditView::new().with_name("m_address").min_width(44))
        .child("Mining threads", EditView::new().with_name("m_threads").fixed_width(8))
        .child("Low CPU priority", Checkbox::new().with_name("m_low"));
    let mut v = LinearLayout::vertical()
        .child(TextView::new("Loading...").with_name("mining_text"))
        .child(DummyView)
        .child(heading("Miner settings ([mining] in the config file)"))
        .child(form)
        .child(DummyView)
        .child(
            LinearLayout::horizontal()
                .child(Button::new("Save", |s| {
                    if save_mining(s) {
                        saved_offer_restart(s);
                    }
                }))
                .child(DummyView)
                .child(Button::new("Reload from file", |s| {
                    let res = app(s).cfg.lock().unwrap().reload();
                    match res {
                        Ok(()) => {
                            fill_mining_form(s);
                            refresh_config_list(s);
                        }
                        Err(e) => info(s, "Reload failed", e),
                    }
                })),
        )
        .child(TextView::new("Changes apply when the node restarts.").style(ColorStyle::secondary()));
    fill_mining_values(&mut v, cfg);
    v.scrollable()
}

fn mining_values(cfg: &ConfigFile) -> (bool, String, String, bool) {
    (
        cfg.get_bool(&["mining", "internal_miner"]).unwrap_or(false),
        cfg.get_str(&["mining", "miner_address"]).unwrap_or_default(),
        cfg.get_int(&["mining", "internal_miner_threads"]).map(|t| t.to_string()).unwrap_or_default(),
        // zebrad's default when the key is absent.
        cfg.get_bool(&["mining", "internal_miner_low_priority"]).unwrap_or(true),
    )
}

fn fill_mining_values(v: &mut LinearLayout, cfg: &ConfigFile) {
    let (enabled, addr, threads, low) = mining_values(cfg);
    v.call_on_name("m_enabled", |c: &mut Checkbox| c.set_checked(enabled));
    v.call_on_name("m_address", |e: &mut EditView| e.set_content(addr));
    v.call_on_name("m_threads", |e: &mut EditView| e.set_content(threads));
    v.call_on_name("m_low", |c: &mut Checkbox| c.set_checked(low));
}

fn fill_mining_form(s: &mut Cursive) {
    let (enabled, addr, threads, low) = mining_values(&app(s).cfg.lock().unwrap());
    s.call_on_name("m_enabled", |c: &mut Checkbox| c.set_checked(enabled));
    s.call_on_name("m_address", |e: &mut EditView| e.set_content(addr));
    s.call_on_name("m_threads", |e: &mut EditView| e.set_content(threads));
    s.call_on_name("m_low", |c: &mut Checkbox| c.set_checked(low));
}

fn save_mining(s: &mut Cursive) -> bool {
    if blocked(s) {
        return false;
    }
    let enabled = checked(s, "m_enabled");
    let low = checked(s, "m_low");
    let address = edit_content(s, "m_address").trim().to_string();
    let threads_txt = edit_content(s, "m_threads");
    let Ok(threads) = threads_txt.trim().parse::<u32>() else {
        info(s, "Mining", format!("Mining threads must be a whole number, not `{threads_txt}`."));
        return false;
    };
    if enabled && address.is_empty() {
        info(s, "Mining", "The internal miner needs a payout address.");
        return false;
    }
    if !address.is_empty() && !address.starts_with('t') {
        info(s, "Mining", "The payout address should be a transparent address (starting with `t`).");
        return false;
    }
    let path = |k: &str| vec!["mining".to_string(), k.to_string()];
    let result = {
        let cfg = app(s).cfg.clone();
        let mut cfg = cfg.lock().unwrap();
        cfg.set(&path("internal_miner"), toml_edit::Value::from(enabled))
            .and_then(|_| cfg.set(&path("miner_address"), toml_edit::Value::from(address)))
            .and_then(|_| cfg.set(&path("internal_miner_threads"), toml_edit::Value::from(i64::from(threads))))
            .and_then(|_| {
                // Leave the key out while it matches zebrad's default (true).
                if low && cfg.get(&["mining", "internal_miner_low_priority"]).is_none() {
                    Ok(())
                } else {
                    cfg.set(&path("internal_miner_low_priority"), toml_edit::Value::from(low))
                }
            })
    };
    if let Err(e) = result {
        info(s, "Mining", e);
        return false;
    }
    refresh_config_list(s);
    let cpus = node::cpu_count() as u32;
    if threads > cpus {
        info(s, "Mining", format!("Note: {threads} threads is more than the {cpus} CPU threads on this host."));
    }
    save_config(s)
}

// ---------------------------------------------------------------------------------------------
// Staking

fn staking_panel(read_only: bool) -> impl View {
    let roster = SelectView::<Option<String>>::new()
        .on_submit(|s, addr: &Option<String>| stake(s, addr.clone()))
        .with_name("roster");
    let bonds = SelectView::<(String, bool)>::new().with_name("bonds");
    if read_only {
        return LinearLayout::vertical()
            .child(TextView::new("Loading...").with_name("staking_text"))
            .child(DummyView)
            .child(heading("Committee: the node's voting roster (get_tfl_roster_zats)"))
            .child(roster.scrollable().max_height(10))
            .child(DummyView)
            .child(heading("My bonds (wallet_staking_positions)"))
            .child(bonds.scrollable().max_height(8))
            .child(DummyView)
            .child(TextView::new(READ_ONLY_NOTE).style(ColorStyle::secondary()))
            .scrollable();
    }
    LinearLayout::vertical()
        .child(TextView::new("Loading...").with_name("staking_text"))
        .child(DummyView)
        .child(heading("Committee: the node's voting roster (get_tfl_roster_zats). Enter stakes to the highlighted finalizer"))
        .child(roster.scrollable().max_height(10))
        .child(DummyView)
        .child(ListView::new().child("Amount (cTAZ)", EditView::new().with_name("stake_amount").fixed_width(18)))
        .child(
            LinearLayout::horizontal()
                .child(Button::new("Stake to selected", |s| {
                    let target = s
                        .call_on_name("roster", |v: &mut SelectView<Option<String>>| v.selection().and_then(|a| (*a).clone()))
                        .flatten();
                    match target {
                        Some(a) => stake(s, Some(a)),
                        None => info(s, "Stake", "Highlight a roster member with a finalizer address first."),
                    }
                }))
                .child(DummyView)
                .child(Button::new("Stake to my node", |s| {
                    let mine = app(s).snap.lock().unwrap().finalizer_address.clone();
                    match mine {
                        Some(a) => stake(s, Some(a)),
                        None => info(s, "Stake", "This node's finalizer address was not found in the journal yet."),
                    }
                }))
                .child(DummyView)
                .child(Button::new("Fill max", fill_max_amount)),
        )
        .child(DummyView)
        .child(heading("My bonds (wallet_staking_positions)"))
        .child(bonds.scrollable().max_height(8))
        .child(
            LinearLayout::horizontal()
                .child(Button::new("Begin unbonding", |s| bond_action(s, "BeginDelegationUnbonding")))
                .child(DummyView)
                .child(Button::new("Withdraw", |s| bond_action(s, "WithdrawDelegationBond")))
                .child(DummyView)
                .child(Button::new("Retarget to selected", |s| bond_action(s, "RetargetDelegationBond")))
                .child(DummyView)
                .child(Button::new("Raw action...", raw_action)),
        )
        .scrollable()
}

fn refresh_roster(s: &mut Cursive, snap: &Snap) {
    let rows = render::roster_rows(snap);
    let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    if labels == app(s).last_roster && !labels.is_empty() {
        return;
    }
    app(s).last_roster = labels;
    s.call_on_name("roster", |v: &mut SelectView<Option<String>>| {
        let keep = v.selected_id().unwrap_or(0);
        v.clear();
        if rows.is_empty() {
            v.add_item("(empty: BFT is not active yet, so there is no roster)", None);
        }
        for r in rows {
            v.add_item(r.label, r.address);
        }
        let _ = v.set_selection(keep.min(v.len().saturating_sub(1)));
    });
}

fn refresh_bonds(s: &mut Cursive, snap: &Snap) {
    let rows = render::bond_rows(snap);
    let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
    if labels == app(s).last_bonds && !labels.is_empty() {
        return;
    }
    app(s).last_bonds = labels;
    s.call_on_name("bonds", |v: &mut SelectView<(String, bool)>| {
        let keep = v.selected_id().unwrap_or(0);
        v.clear();
        if rows.is_empty() {
            v.add_item("(no bonds yet)", (String::new(), false));
        }
        for r in rows {
            v.add_item(r.label, (r.bond_key, r.withdrawable));
        }
        let _ = v.set_selection(keep.min(v.len().saturating_sub(1)));
    });
}

fn available_zats(snap: &Snap) -> u64 {
    let w = &snap.wallet;
    let get = |k: &str| w.get(k).and_then(Value::as_u64).unwrap_or(0);
    get("user_shielded_spendable_zats") + get("user_unshielded_zats")
}

fn fill_max_amount(s: &mut Cursive) {
    let avail = available_zats(&app(s).snap.lock().unwrap());
    let amount = avail.saturating_sub(STAKE_FEE_BUFFER_ZATS);
    let text = format!("{}.{:08}", amount / 100_000_000, amount % 100_000_000);
    s.call_on_name("stake_amount", |e: &mut EditView| e.set_content(text));
}

fn parse_ctaz(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let (whole, frac) = t.split_once('.').unwrap_or((t, ""));
    if frac.len() > 8 || whole.is_empty() && frac.is_empty() {
        return Err(format!("`{t}` is not an amount (up to 8 decimals)"));
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| format!("`{t}` is not an amount"))? };
    let frac: u64 = format!("{frac:0<8}").parse().map_err(|_| format!("`{t}` is not an amount"))?;
    let z = whole.checked_mul(100_000_000).and_then(|w| w.checked_add(frac)).ok_or("amount too large")?;
    if z == 0 {
        return Err("the amount must be more than zero".into());
    }
    Ok(z)
}

fn window_open(s: &mut Cursive) -> bool {
    let h = app(s).snap.lock().unwrap().chain.get("blocks").and_then(Value::as_u64).unwrap_or(0);
    let (open, text) = render::staking_window(h);
    if !open {
        info(s, "Staking window", format!("Staking actions are only accepted while the window is open. It is {text}."));
    }
    open
}

fn stake(s: &mut Cursive, target: Option<String>) {
    if blocked(s) {
        return;
    }
    let Some(target) = target else {
        info(s, "Stake", "That roster entry has no finalizer address.");
        return;
    };
    let amount = match parse_ctaz(&edit_content(s, "stake_amount")) {
        Ok(z) => z,
        Err(e) => return info(s, "Stake", e),
    };
    let avail = available_zats(&app(s).snap.lock().unwrap());
    if amount > avail {
        return info(s, "Stake", format!("Only {} is available.", render::zats(Some(avail))));
    }
    if !window_open(s) {
        return;
    }
    let action = json!({"CreateNewDelegationBond": {"amount_zats": amount, "target_finalizer": target}});
    confirm_staking(s, format!("Bond {} to\n{target}?", render::zats(Some(amount))), action);
}

fn bond_action(s: &mut Cursive, kind: &str) {
    if blocked(s) {
        return;
    }
    let sel = s
        .call_on_name("bonds", |v: &mut SelectView<(String, bool)>| v.selection().map(|x| (*x).clone()))
        .flatten();
    let Some((bond_key, withdrawable)) = sel.filter(|(k, _)| !k.is_empty()) else {
        return info(s, "Bonds", "Highlight one of your bonds first.");
    };
    if kind == "WithdrawDelegationBond" && !withdrawable {
        return info(s, "Withdraw", "That bond is still active. Begin unbonding first; it becomes withdrawable later.");
    }
    if kind != "WithdrawDelegationBond" && withdrawable {
        return info(s, "Bonds", "That bond is already unbonded; it can only be withdrawn.");
    }
    if !window_open(s) {
        return;
    }
    let action = if kind == "RetargetDelegationBond" {
        let target = s
            .call_on_name("roster", |v: &mut SelectView<Option<String>>| v.selection().and_then(|a| (*a).clone()))
            .flatten();
        let Some(target) = target else {
            return info(s, "Retarget", "Highlight the new finalizer in the roster first.");
        };
        json!({kind: {"bond_key": bond_key, "target_finalizer": target}})
    } else {
        json!({kind: {"bond_key": bond_key}})
    };
    confirm_staking(s, format!("{kind} for bond {}?", render::short(&bond_key, 16)), action);
}

fn raw_action(s: &mut Cursive) {
    if blocked(s) {
        return;
    }
    let mine = app(s).snap.lock().unwrap().finalizer_address.clone().unwrap_or("<zfin address>".into());
    let templates = vec![
        ("CreateNewDelegationBond", json!({"CreateNewDelegationBond": {"amount_zats": 1_000_000_000u64, "target_finalizer": mine}})),
        ("RetargetDelegationBond", json!({"RetargetDelegationBond": {"bond_key": "<bond key hex>", "target_finalizer": "<zfin address>"}})),
        ("BeginDelegationUnbonding", json!({"BeginDelegationUnbonding": {"bond_key": "<bond key hex>"}})),
        ("WithdrawDelegationBond", json!({"WithdrawDelegationBond": {"bond_key": "<bond key hex>"}})),
    ];
    let mut pick = SelectView::<String>::new().popup();
    for (name, v) in &templates {
        pick.add_item(*name, serde_json::to_string_pretty(v).unwrap());
    }
    pick.set_on_submit(|s, text: &String| {
        let text = text.clone();
        s.call_on_name("raw_json", |t: &mut TextArea| t.set_content(text));
    });
    let first = serde_json::to_string_pretty(&templates[0].1).unwrap();
    s.add_layer(
        Dialog::around(
            LinearLayout::vertical()
                .child(TextView::new("Template:"))
                .child(pick)
                .child(DummyView)
                .child(TextArea::new().content(first).with_name("raw_json").min_size((70, 8))),
        )
        .title("Staking action (sent to staking_command)")
        .button("Submit", |s| {
            let text = s.call_on_name("raw_json", |t: &mut TextArea| t.get_content().to_string()).unwrap_or_default();
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => {
                    s.pop_layer();
                    confirm_staking(s, "Send this staking action?".into(), v);
                }
                Err(e) => info(s, "Invalid JSON", e.to_string()),
            }
        })
        .dismiss_button("Cancel"),
    );
}

fn confirm_staking(s: &mut Cursive, question: String, action: Value) {
    let body = format!("{question}\n\n{}", serde_json::to_string_pretty(&action).unwrap());
    s.add_layer(
        Dialog::text(body)
            .title("Confirm staking action")
            .button("Submit", move |s| {
                s.pop_layer();
                submit_staking(s, action.clone());
            })
            .dismiss_button("Cancel")
            .with(|d| { d.set_focus(DialogFocus::Button(1)); }),
    );
}

fn submit_staking(s: &mut Cursive, action: Value) {
    if blocked(s) {
        return;
    }
    let rpc = app(s).rpc.clone();
    let sink = s.cb_sink().clone();
    s.add_layer(
        Dialog::text("Building the transaction in the node wallet.\nThis can take a few minutes.")
            .title("Submitting")
            .with_name("busy"),
    );
    std::thread::spawn(move || {
        // The RPC returns once the wallet has built (and queued) the transaction.
        let res = rpc.call_with_timeout("staking_command", json!([action.to_string()]), Duration::from_secs(600));
        let _ = sink.send(Box::new(move |s: &mut Cursive| {
            if s.find_name::<Dialog>("busy").is_some() {
                s.pop_layer();
            }
            match res {
                Ok(_) => info(s, "Staking action sent", "The node accepted the action. Watch Staked / My bonds update once it is mined."),
                Err(e) => info(s, "Staking action failed", e),
            }
        }));
    });
}

// ---------------------------------------------------------------------------------------------
// Config

fn config_panel(read_only: bool) -> impl View {
    if read_only {
        return LinearLayout::vertical()
            .child(TextView::new("").with_name("config_status"))
            .child(DummyView)
            .child(
                SelectView::<usize>::new()
                    .on_submit(|s, i: &usize| edit_entry(s, *i))
                    .with_name("config_list")
                    .scrollable()
                    .full_height(),
            )
            .child(DummyView)
            .child(
                LinearLayout::horizontal()
                    .child(Button::new("Reload", |s| {
                        let res = app(s).cfg.lock().unwrap().reload();
                        match res {
                            Ok(()) => {
                                refresh_config_list(s);
                                refresh(s);
                            }
                            Err(e) => info(s, "Reload failed", e),
                        }
                    }))
                    .child(DummyView)
                    .child(TextView::new(READ_ONLY_NOTE).style(ColorStyle::secondary())),
            );
    }
    LinearLayout::vertical()
        .child(TextView::new("").with_name("config_status"))
        .child(DummyView)
        .child(
            SelectView::<usize>::new()
                .on_submit(|s, i: &usize| edit_entry(s, *i))
                .with_name("config_list")
                .scrollable()
                .full_height(),
        )
        .child(DummyView)
        .child(
            LinearLayout::horizontal()
                .child(Button::new("Edit", |s| {
                    if let Some(i) = selected_entry(s) {
                        edit_entry(s, i)
                    }
                }))
                .child(DummyView)
                .child(Button::new("Add key", add_entry))
                .child(DummyView)
                .child(Button::new("Remove", remove_entry))
                .child(DummyView)
                .child(Button::new("Save", |s| {
                    if save_config(s) {
                        saved_offer_restart(s);
                    }
                }))
                .child(DummyView)
                .child(Button::new("Reload", |s| {
                    let res = app(s).cfg.lock().unwrap().reload();
                    match res {
                        Ok(()) => {
                            refresh_config_list(s);
                            fill_mining_form(s);
                            refresh(s);
                        }
                        Err(e) => info(s, "Reload failed", e),
                    }
                }))
                .child(DummyView)
                .child(Button::new("Open in $EDITOR", open_editor)),
        )
}

fn config_status(path: &str, dirty: bool) -> String {
    format!(
        "File: {path}{}\nEnter edits the highlighted value. Values keep their type; the file keeps its comments.\n\
         Saving keeps a timestamped backup next to the file. Changes apply when the node restarts.",
        if dirty { "   [unsaved changes]" } else { "" }
    )
}

fn refresh_config_list(s: &mut Cursive) {
    let entries = app(s).cfg.lock().unwrap().entries();
    s.call_on_name("config_list", |v: &mut SelectView<usize>| {
        let keep = v.selected_id().unwrap_or(0);
        v.clear();
        for (i, e) in entries.iter().enumerate() {
            v.add_item(format!("{:<44} = {}", e.key(), e.value), i);
        }
        let _ = v.set_selection(keep.min(v.len().saturating_sub(1)));
    });
    refresh(s);
}

fn selected_entry(s: &mut Cursive) -> Option<usize> {
    s.call_on_name("config_list", |v: &mut SelectView<usize>| v.selection().map(|i| *i)).flatten()
}

fn edit_entry(s: &mut Cursive, i: usize) {
    if blocked(s) {
        return;
    }
    let entries = app(s).cfg.lock().unwrap().entries();
    let Some(e) = entries.get(i) else { return };
    if !e.editable {
        return info(s, "Config", format!("{} is an array of tables; edit it with `Open in $EDITOR`.", e.key()));
    }
    let path = e.path.clone();
    let current = {
        let cfg = app(s).cfg.clone();
        let cfg = cfg.lock().unwrap();
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        cfg.get(&refs).map(config::edit_text).unwrap_or_default()
    };
    s.add_layer(
        Dialog::around(EditView::new().content(current).with_name("cfg_value").min_width(50))
            .title(e.key())
            .button("OK", move |s| {
                let input = edit_content(s, "cfg_value");
                let res = {
                    let cfg = app(s).cfg.clone();
                    let mut cfg = cfg.lock().unwrap();
                    let refs: Vec<&str> = path.iter().map(String::as_str).collect();
                    config::parse_value(&input, cfg.get(&refs)).and_then(|v| cfg.set(&path, v))
                };
                match res {
                    Ok(()) => {
                        s.pop_layer();
                        refresh_config_list(s);
                        fill_mining_form(s);
                    }
                    Err(e) => info(s, "Invalid value", e),
                }
            })
            .dismiss_button("Cancel"),
    );
}

fn add_entry(s: &mut Cursive) {
    if blocked(s) {
        return;
    }
    s.add_layer(
        Dialog::around(
            ListView::new()
                .child("Key (section.key)", EditView::new().with_name("new_key").min_width(40))
                .child("Value (TOML)", EditView::new().with_name("new_value").min_width(40)),
        )
        .title("Add config key")
        .button("Add", |s| {
            let key = edit_content(s, "new_key");
            let value = edit_content(s, "new_value");
            let path: Vec<String> = key.trim().split('.').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
            if path.len() < 2 {
                return info(s, "Add key", "Use the form section.key, for example mining.internal_miner_threads.");
            }
            let res = {
                let cfg = app(s).cfg.clone();
                let mut cfg = cfg.lock().unwrap();
                config::parse_value(&value, None).and_then(|v| cfg.set(&path, v))
            };
            match res {
                Ok(()) => {
                    s.pop_layer();
                    refresh_config_list(s);
                    fill_mining_form(s);
                }
                Err(e) => info(s, "Add key", e),
            }
        })
        .dismiss_button("Cancel"),
    );
}

fn remove_entry(s: &mut Cursive) {
    if blocked(s) {
        return;
    }
    let Some(i) = selected_entry(s) else { return };
    let entries = app(s).cfg.lock().unwrap().entries();
    let Some(e) = entries.get(i) else { return };
    let path = e.path.clone();
    s.add_layer(
        Dialog::text(format!("Remove `{}`? zebrad will use its built-in default.", e.key()))
            .title("Remove key")
            .button("Remove", move |s| {
                let res = app(s).cfg.lock().unwrap().remove(&path);
                s.pop_layer();
                match res {
                    Ok(()) => {
                        refresh_config_list(s);
                        fill_mining_form(s);
                    }
                    Err(e) => info(s, "Remove", e),
                }
            })
            .dismiss_button("Cancel"),
    );
}

/// Saves the shared config document. Refuses to overwrite a file someone else changed.
fn save_config(s: &mut Cursive) -> bool {
    if blocked(s) {
        return false;
    }
    let cfg = app(s).cfg.clone();
    if cfg.lock().unwrap().changed_on_disk() {
        s.add_layer(
            Dialog::text("The config file changed on disk since it was loaded. Saving would overwrite those changes.")
                .title("Config changed on disk")
                .button("Reload (discard my edits)", |s| {
                    s.pop_layer();
                    let res = app(s).cfg.lock().unwrap().reload();
                    if let Err(e) = res {
                        info(s, "Reload failed", e);
                    }
                    refresh_config_list(s);
                    fill_mining_form(s);
                })
                .dismiss_button("Cancel"),
        );
        return false;
    }
    let res = cfg.lock().unwrap().save();
    refresh_config_list(s);
    match res {
        Ok(_) => true,
        Err(e) => {
            info(s, "Save failed", e);
            false
        }
    }
}

fn saved_offer_restart(s: &mut Cursive) {
    if !app(s).snap.lock().unwrap().managed {
        return info(s, "Saved", "Config saved (previous version kept as a .bak file). Restart zebrad so it takes effect.");
    }
    s.add_layer(
        Dialog::text("Config saved (previous version kept as a .bak file).\nRestart the node now so it takes effect?")
            .title("Saved")
            .button("Quick restart", |s| {
                s.pop_layer();
                quick_restart(s);
            })
            .button("sudo restart", |s| {
                s.pop_layer();
                run_sudo(s, "restart");
            })
            .dismiss_button("Later")
            .with(|d| { d.set_focus(DialogFocus::Button(2)); }),
    );
}

fn open_editor(s: &mut Cursive) {
    if blocked(s) {
        return;
    }
    let go = |s: &mut Cursive| {
        let editor = node::default_editor();
        let path = app(s).cfg.lock().unwrap().path.display().to_string();
        let mut cmd: Vec<String> = editor.split_whitespace().map(String::from).collect();
        cmd.push(path);
        *app(s).pending.lock().unwrap() = Some(Pending { cmd, reload_config: true });
        s.quit();
    };
    if app(s).cfg.lock().unwrap().dirty {
        s.add_layer(
            Dialog::text("Unsaved edits in the TUI will be discarded when the file is reloaded after editing.")
                .title("Open in editor")
                .button("Continue", move |s| {
                    s.pop_layer();
                    go(s)
                })
                .dismiss_button("Cancel"),
        );
    } else {
        go(s);
    }
}

// ---------------------------------------------------------------------------------------------
// Node control and logs

fn control_panel(read_only: bool) -> impl View {
    if read_only {
        return LinearLayout::vertical()
            .child(TextView::new("Loading...").with_name("control_text"))
            .child(DummyView)
            .child(TextView::new(READ_ONLY_NOTE).style(ColorStyle::secondary()))
            .scrollable();
    }
    LinearLayout::vertical()
        .child(TextView::new("Loading...").with_name("control_text"))
        .child(DummyView)
        .child(
            LinearLayout::horizontal()
                .child(Button::new("Start", |s| confirm(s, "Start the node (sudo systemctl start)?", |s| run_sudo(s, "start"))))
                .child(DummyView)
                .child(Button::new("Stop", |s| confirm(s, "Stop the node (sudo systemctl stop)?", |s| run_sudo(s, "stop"))))
                .child(DummyView)
                .child(Button::new("Restart", |s| confirm(s, "Restart the node (sudo systemctl restart)?", |s| run_sudo(s, "restart"))))
                .child(DummyView)
                .child(Button::new("Quick restart (no sudo)", |s| confirm(s, "Send SIGTERM and let systemd restart the node?", quick_restart))),
        )
        .scrollable()
}

fn confirm(s: &mut Cursive, question: &str, then: fn(&mut Cursive)) {
    s.add_layer(
        Dialog::text(question)
            .title("Confirm")
            .button("Yes", move |s| {
                s.pop_layer();
                then(s);
            })
            .dismiss_button("No")
            .with(|d| { d.set_focus(DialogFocus::Button(1)); }),
    );
}

fn managed(s: &mut Cursive) -> bool {
    let m = app(s).snap.lock().unwrap().managed;
    if !m {
        info(s, "Node control", "The node is not running as a systemd service, so the TUI cannot start, stop or restart it. Do that where you run zebrad.");
    }
    m
}

fn run_sudo(s: &mut Cursive, op: &str) {
    if blocked(s) {
        return;
    }
    if !managed(s) {
        return;
    }
    let service = app(s).service.clone();
    let cmd = ["sudo", "systemctl", op, &service].iter().map(|x| x.to_string()).collect();
    *app(s).pending.lock().unwrap() = Some(Pending { cmd, reload_config: false });
    s.quit();
}

fn quick_restart(s: &mut Cursive) {
    if blocked(s) {
        return;
    }
    if !managed(s) {
        return;
    }
    let (pid, policy) = {
        let snap = app(s).snap.lock().unwrap();
        (snap.service.get("MainPID").cloned().unwrap_or_default(), snap.service.get("Restart").cloned().unwrap_or_default())
    };
    if policy != "always" && policy != "on-failure" && policy != "on-abnormal" {
        return info(s, "Quick restart", format!("systemd restart policy is `{policy}`, so the node would stay down. Use Restart (sudo)."));
    }
    match node::quick_restart(&pid) {
        Ok(()) => info(s, "Quick restart", "SIGTERM sent. The node shuts down cleanly and systemd starts it again after RestartSec."),
        Err(e) => info(s, "Quick restart", e),
    }
}

fn logs_panel() -> impl View {
    let mut filter = SelectView::<usize>::new().popup();
    for (i, name) in render::LOG_FILTERS.iter().enumerate() {
        filter.add_item(*name, i);
    }
    filter.set_on_submit(|s, i: &usize| {
        app(s).log_filter = *i;
        refresh(s);
    });
    let noise = Checkbox::new().checked().on_change(|s, on| {
        app(s).hide_noise = on;
        refresh(s);
    });
    LinearLayout::vertical()
        .child(
            LinearLayout::horizontal()
                .child(TextView::new("Show: "))
                .child(filter)
                .child(DummyView)
                .child(noise)
                .child(TextView::new(" hide repeated sync chatter")),
        )
        .child(DummyView)
        .child(
            ScrollView::new(TextView::new("Loading...").with_name("logs_text"))
                .scroll_strategy(ScrollStrategy::StickToBottom)
                .full_screen(),
        )
}
