// ESNODE | Source Available BUSL-1.1 | Copyright (c) 2024 Estimatedstocks AB
use std::io::{stdout, Stdout};
use std::time::{Duration, Instant};

use agent_core::state::{GpuStatus, StatusSnapshot};
use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};

use crate::client::AgentClient;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    MainMenu,
    NodeOverview,
    GpuPower,
    NetworkDisk,
    Efficiency,
    MetricsProfiles,
    AgentStatus,
    ConnectServer,
}

#[derive(Clone, Debug)]
pub struct ManagedMetadata {
    pub server: Option<String>,
    pub cluster_id: Option<String>,
    pub node_id: Option<String>,
    pub last_contact_unix_ms: Option<u64>,
    pub state: String,
}

#[derive(Clone, Debug)]
pub enum AgentMode {
    Standalone,
    Managed(ManagedMetadata),
}

struct AppState {
    screen: Screen,
    last_status: Option<StatusSnapshot>,
    message: Option<String>,
    no_color: bool,
    should_exit: bool,
    mode: AgentMode,
    config: agent_core::AgentConfig,
}

impl AppState {
    fn new(no_color: bool, mode: AgentMode, config: agent_core::AgentConfig) -> Self {
        AppState {
            screen: Screen::MainMenu,
            last_status: None,
            message: None,
            no_color,
            should_exit: false,
            mode,
            config,
        }
    }

    fn set_status(&mut self, status: Option<StatusSnapshot>) {
        self.last_status = status;
    }

    fn set_screen(&mut self, screen: Screen) {
        self.screen = screen;
        self.message = None;
    }

    fn back(&mut self) {
        if self.screen == Screen::MainMenu {
            self.should_exit = true;
        } else {
            self.screen = Screen::MainMenu;
        }
    }
}

pub fn run_console(
    client: &AgentClient,
    no_color: bool,
    mode: AgentMode,
    config: agent_core::AgentConfig,
) -> Result<()> {
    let stdout = prepare_terminal()?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    terminal.show_cursor()?;

    let mut state = AppState::new(no_color, mode, config);
    refresh_status(&mut state, client);
    let mut last_refresh = Instant::now();

    loop {
        terminal.draw(|f| render(f, &state))?;

        if state.should_exit {
            break;
        }

        let timeout = Duration::from_millis(200);
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let refresh_now = handle_key(key.code, &mut state);
                    if state.should_exit {
                        break;
                    }
                    match key.code {
                        KeyCode::Char('1') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::NodeOverview)
                        }
                        KeyCode::Char('2') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::GpuPower)
                        }
                        KeyCode::Char('3') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::NetworkDisk)
                        }
                        KeyCode::Char('4') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::Efficiency)
                        }
                        KeyCode::Char('5') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::MetricsProfiles)
                        }
                        KeyCode::Char('6') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::AgentStatus)
                        }
                        KeyCode::Char('7') if state.screen == Screen::MainMenu => {
                            state.set_screen(Screen::ConnectServer)
                        }
                        _ => {}
                    }
                    if refresh_now {
                        refresh_status(&mut state, client);
                    }
                }
            }
        }

        if last_refresh.elapsed() > Duration::from_secs(5) {
            refresh_status(&mut state, client);
            last_refresh = Instant::now();
        }
    }

    restore_terminal()?;
    Ok(())
}

fn refresh_status(state: &mut AppState, client: &AgentClient) {
    match client.fetch_status() {
        Ok(snapshot) => {
            state.message = None;
            state.set_status(Some(snapshot));
        }
        Err(err) => {
            state.message = Some(format!(
                "Unable to reach agent at {}: {err}",
                client.base_url()
            ));
            state.set_status(None);
        }
    }
}

fn prepare_terminal() -> Result<Stdout> {
    enable_raw_mode().context("enabling raw mode")?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, cursor::Show).context("preparing terminal")?;
    Ok(stdout)
}

fn restore_terminal() -> Result<()> {
    let mut stdout = stdout();
    execute!(stdout, LeaveAlternateScreen, cursor::Show).context("restoring terminal")?;
    disable_raw_mode().context("disabling raw mode")
}

fn render(frame: &mut ratatui::Frame, state: &AppState) {
    // Use full terminal area instead of a fixed 80x24 window so the console scales
    // with the current terminal size.
    let area = frame.size();
    if let AgentMode::Managed(_) = state.mode {
        render_managed(frame, area, state);
        return;
    }
    match state.screen {
        Screen::MainMenu => render_main_menu(frame, area, state),
        Screen::NodeOverview => render_node_overview(frame, area, state),
        Screen::GpuPower => render_gpu_power(frame, area, state),
        Screen::NetworkDisk => render_network_disk(frame, area, state),
        Screen::Efficiency => render_efficiency(frame, area, state),
        Screen::MetricsProfiles => render_metric_profiles(frame, area, state),
        Screen::AgentStatus => render_agent_status(frame, area, state),
        Screen::ConnectServer => render_connect_server(frame, area, state),
    }
}

fn render_main_menu(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let mode_line = match &state.mode {
        AgentMode::Standalone => "STANDALONE".to_string(),
        AgentMode::Managed(_) => "MANAGED".to_string(),
    };
    let server_line = match &state.mode {
        AgentMode::Standalone => "(not connected)".to_string(),
        AgentMode::Managed(meta) => meta
            .server
            .as_ref()
            .cloned()
            .unwrap_or_else(|| "(unknown)".to_string()),
    };
    let text = vec![
        Line::from("                          ESNODE – CORE CONSOLE                         N01"),
        Line::from("                        Estimatedstocks AB – ESNODE-Core                "),
        Line::from(""),
        Line::from(format!(
            "   Core Mode  . . . . . . . . . . . . . . . :  {}",
            mode_line
        )),
        Line::from(format!(
            "   Server (Pulse)  . . . . . . . . . . . .  :  {}",
            server_line
        )),
        Line::from(""),
        Line::from("   Select one of the following options and press Enter:"),
        Line::from(""),
        Line::from("     1. ESNODE Overview          (CPU / Memory / Load)"),
        Line::from("     2. GPU & Power              (GPU, VRAM, watts, thermals)"),
        Line::from("     3. Network & Disk           (I/O, bandwidth, latency)"),
        Line::from("     4. Efficiency & MCP Signals (tokens-per-watt, routing scores)"),
        Line::from("     5. Metrics Profiles         (enable/disable metric sets)"),
        Line::from("     6. Agent Status & Logs      (health, errors, config)"),
        Line::from("     7. Connect to ESNODE-Pulse (attach this ESNODE to a cluster)"),
        Line::from(""),
        Line::from("     Selection . . . . . . . . . . . . . . . . . .  __"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=Node Info   F10=Help   F12=Cancel"),
    ];
    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .alignment(Alignment::Left)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
    // Place a visible cursor on the selection line so users can see the active input spot.
    let selection_row = area.y.saturating_add(16);
    let selection_col = area.x.saturating_add(50);
    frame.set_cursor(selection_col, selection_row);
    if let Some(msg) = &state.message {
        render_message(frame, area, msg, state);
    }
}

fn render_node_overview(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for metrics from esnode-core daemon...",
        );
        return;
    }
    let summary = NodeSummary::from_status(state);
    let text = vec![
        Line::from("                            ESNODE – NODE OVERVIEW                        N01"),
        Line::from(format!(
            " Node: {node:<18} Region: {region:<16} Uptime: {uptime:<12}",
            node = summary.node_name,
            region = summary.region,
            uptime = summary.uptime
        )),
        Line::from(""),
        Line::from(format!(
            "   CPU:   {cores:<8} Load(1/5/15):  {l1:<4} {l5:<4} {l15:<4}     Util:  {util:>6}",
            cores = summary.cores,
            l1 = summary.load_1,
            l5 = summary.load_5,
            l15 = summary.load_15,
            util = summary.cpu_util
        )),
        Line::from(format!(
            "   Mem:   {mem_total:<9} Used:  {mem_used:<10} Free:  {mem_free:<10} Swap Used:  {swap_used}",
            mem_total = summary.mem_total,
            mem_used = summary.mem_used,
            mem_free = summary.mem_free,
            swap_used = summary.swap_used
        )),
        Line::from(format!(
            "   Disk:  /           Used:  {disk_used:<12} IO Latency:  {disk_lat}",
            disk_used = summary.disk_used,
            disk_lat = summary.disk_latency
        )),
        Line::from(format!(
            "   Net:   eth0        Rx:  {net_rx:<8}   Tx:  {net_tx:<8}   Drops:  {net_drop}",
            net_rx = summary.net_rx,
            net_tx = summary.net_tx,
            net_drop = summary.net_drop
        )),
        Line::from(""),
        Line::from(format!(
            "   Power: Node Draw:  {power_draw:<8}   Limit:  {power_limit:<8}   Spikes (24h):  {spikes}",
            power_draw = summary.node_power,
            power_limit = summary.node_limit,
            spikes = summary.spikes
        )),
        Line::from(format!(
            "   Therm: Inlet:  {inlet:<6} Exhaust:  {exhaust:<6}      CPU Hotspot:  {hotspot}",
            inlet = summary.therm_inlet,
            exhaust = summary.therm_exhaust,
            hotspot = summary.therm_hotspot
        )),
        Line::from(""),
        Line::from(format!(
            "   GPUs:  {count:<2} detected     Total VRAM:  {vram:<6}",
            count = summary.gpu_count,
            vram = summary.total_vram,
        )),
        Line::from(format!(
            "          Avg Util:  {util:<6}     Avg Power:  {gpu_power:<8}     Tokens/Watt:  {tokens}",
            util = summary.avg_gpu_util,
            gpu_power = summary.avg_gpu_power,
            tokens = summary.tokens_per_watt
        )),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=GPU Detail   F10=Metrics Profile   F12=Menu"),
    ];

    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .style(primary_style(state))
        .alignment(Alignment::Left)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
    if let Some(msg) = &state.message {
        render_message(frame, area, msg, state);
    }
}

fn render_gpu_power(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for GPU/power data from esnode-core daemon...",
        );
        return;
    }
    let lines = build_gpu_table(state.last_status.as_ref());
    let text = vec![
        Line::from("                          ESNODE – GPU & POWER STATUS                    N01"),
        Line::from(""),
    ]
    .into_iter()
    .chain(lines)
    .chain(vec![
        Line::from(""),
        Line::from("    Option . . . . . . . . . . . . . .  __   (1=GPU Detail, 2=Power Spikes, 3=KV Cache)"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=Power Spikes   F11=More Fields   F12=Back"),
    ])
    .collect::<Vec<_>>();

    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .style(primary_style(state))
        .alignment(Alignment::Left)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
    if let Some(msg) = &state.message {
        render_message(frame, area, msg, state);
    }
}

fn render_network_disk(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for network/disk data from esnode-core daemon...",
        );
        return;
    }
    let text = vec![
        Line::from("                        ESNODE – NETWORK & DISK STATUS                   N01"),
        Line::from(""),
        Line::from(" Network Interfaces:"),
        Line::from("   IF   State   Rx MB/s  Tx MB/s  Rx Err  Tx Err  Drops"),
        Line::from("   ---  ------  -------- -------- ------- ------- -----"),
        Line::from("   eth0 UP      n/a      n/a      0       0       0"),
        Line::from("   eth1 DOWN    0.0      0.0      0       0       0"),
        Line::from(""),
        Line::from(" Disks:"),
        Line::from("   Mount   FS Type  Used / Total        Read MB/s  Write MB/s  Latency ms"),
        Line::from("   ------  -------  ----------------    ---------- ----------- ----------"),
        Line::from("   /       ext4     n/a                n/a        n/a        n/a"),
        Line::from("   /data   xfs      n/a                n/a        n/a        n/a"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=I/O Detail   F12=Back"),
    ];
    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_efficiency(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for efficiency metrics from esnode-core daemon...",
        );
        return;
    }
    let summary = NodeSummary::from_status(state);
    let text = vec![
        Line::from("                     ESNODE – EFFICIENCY & MCP SIGNALS                   N01"),
        Line::from(""),
        Line::from("   Efficiency (Last 5 minutes):"),
        Line::from(format!(
            "     Tokens per Joule . . . . . . . . . . . . . . . . :  {}",
            summary.tokens_per_joule
        )),
        Line::from("     Tokens per Watt-second  . . . . . . . . . . . . :  n/a"),
        Line::from("     Inference cost per 1M tokens (USD est.) . . . . :  n/a"),
        Line::from("     Utilization score (0–100)  . . . . . . . . . . :  83"),
        Line::from(""),
        Line::from("   Routing / Scheduling Scores:"),
        Line::from("     Best-fit GPU score  . . . . . . . . . . . . . . :  0.91"),
        Line::from("     Energy cost score . . . . . . . . . . . . . . . :  0.23"),
        Line::from("     Thermal risk score  . . . . . . . . . . . . . . :  0.12"),
        Line::from("     Memory pressure score . . . . . . . . . . . . . :  0.37"),
        Line::from("     Cache freshness score . . . . . . . . . . . . . :  0.88"),
        Line::from(""),
        Line::from("   Batch & Queue:"),
        Line::from("     Batch capacity free (%)  . . . . . . . . . . . . :  28.5"),
        Line::from("     KV cache free bytes  . . . . . . . . . . . . . . :  54.3 GiB"),
        Line::from("     Inference queue length  . . . . . . . . . . . . :  12"),
        Line::from("     Speculative ready flag . . . . . . . . . . . . . :  YES"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=Explain Scores   F12=Back"),
    ];
    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_metric_profiles(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for metrics profile state from esnode-core daemon...",
        );
        return;
    }
    let summary = MetricToggleState::from_status(state.last_status.as_ref());
    let text = vec![
        Line::from("                         ESNODE – METRICS PROFILES                      N01"),
        Line::from(""),
        Line::from("   Current Metrics Sets (Y=enabled, N=disabled):"),
        Line::from(""),
        Line::from(format!(
            "     Host / Node (CPU, mem, disk, net) . . . . . . . [{}]",
            summary.host
        )),
        Line::from(format!(
            "     GPU Core (util, VRAM, temp) . . . . . . . . . . [{}]",
            summary.gpu_core
        )),
        Line::from(format!(
            "     GPU Power & Energy  . . . . . . . . . . . . . . [{}]",
            summary.gpu_power
        )),
        Line::from(format!(
            "     MCP Efficiency & Routing . . . . . . . . . . . .[{}]",
            summary.mcp
        )),
        Line::from(format!(
            "     Application / HTTP Metrics . . . . . . . . . . .[{}]",
            summary.app
        )),
        Line::from(format!(
            "     Rack / Room Thermals (BMC/IPMI) . . . . . . . . [{}]",
            summary.rack
        )),
        Line::from(""),
        Line::from("   Option:"),
        Line::from("     1=Toggle Host/Node"),
        Line::from("     2=Toggle GPU Core"),
        Line::from("     3=Toggle GPU Power/Energy"),
        Line::from("     4=Toggle MCP Metrics"),
        Line::from("     5=Toggle Application Metrics"),
        Line::from("     6=Toggle Rack/Room Thermals"),
        Line::from(""),
        Line::from("   Selection . . . . . . . . . . . . . . . . . . . . __"),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F10=Save Now   F12=Back"),
    ];
    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(text)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_agent_status(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    if state.last_status.is_none() {
        render_placeholder(
            frame,
            area,
            state,
            "Waiting for agent status from esnode-core daemon...",
        );
        return;
    }
    let errors = state
        .last_status
        .as_ref()
        .map(|s| s.last_errors.clone())
        .unwrap_or_default();
    let mut lines = vec![
        Line::from("                       ESNODE – AGENT STATUS & LOGS                     N01"),
        Line::from(""),
        Line::from("   Agent Status:"),
        Line::from(format!(
            "     Running . . . . . . . . . . . . . . . . . . . . :  {}",
            state
                .last_status
                .as_ref()
                .map(|s| if s.healthy { "YES" } else { "WARN" })
                .unwrap_or("UNKNOWN")
        )),
        Line::from(format!(
            "     Last scrape (unix ms) . . . . . . . . . . . . . :  {}",
            state
                .last_status
                .as_ref()
                .map(|s| s.last_scrape_unix_ms.to_string())
                .unwrap_or_else(|| "n/a".to_string())
        )),
        Line::from(format!(
            "     Node power (W) . . . . . . . . . . . . . . . . .:  {}",
            state
                .last_status
                .as_ref()
                .and_then(|s| s.node_power_watts)
                .map(|v| format!("{v:.1}"))
                .unwrap_or_else(|| "n/a".to_string())
        )),
        Line::from(""),
        Line::from("   Recent Errors (last 10):"),
    ];

    if errors.is_empty() {
        lines.push(Line::from("     none"));
    } else {
        for (idx, err) in errors.iter().enumerate() {
            lines.push(Line::from(format!(
                "     {}. [{}] {} (unix_ms={})",
                idx + 1,
                err.collector,
                err.message,
                err.unix_ms
            )));
        }
    }

    lines.extend_from_slice(&[
        Line::from(""),
        Line::from("   Option:"),
        Line::from("     1=View full log (last 100 lines)"),
        Line::from("     2=Export diagnostics snapshot"),
        Line::from("     3=Show config"),
        Line::from(""),
        Line::from("   Selection . . . . . . . . . . . . . . . . . . . . __"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F9=Diagnostics   F12=Back"),
    ]);

    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(lines)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_connect_server(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let lines = vec![
        Line::from("                    ESNODE – CONNECT TO ESNODE-SERVER                    N02"),
        Line::from(""),
        Line::from("   This node is currently running in STANDALONE mode."),
        Line::from("   To enroll it into a managed cluster, enter the ESNODE-Pulse details."),
        Line::from(""),
        Line::from("   Server address (host:port)  . . . . . . . . . . . . .  __________________"),
        Line::from("   Join token (optional)  . . . . . . . . . . . . . . . .  __________________"),
        Line::from(""),
        Line::from("   After connection:"),
        Line::from("     - Local tuning via this console will be disabled."),
        Line::from("     - Monitoring, alerts and throttling will be controlled centrally"),
        Line::from("       from the ESNODE-Pulse."),
        Line::from("     - Local /metrics endpoint and Prometheus output remain active."),
        Line::from(""),
        Line::from("   Option:"),
        Line::from("     1=Connect Now    2=Test Connection    3=Cancel"),
        Line::from(""),
        Line::from("   Selection . . . . . . . . . . . . . . . . . . . . . __"),
        Line::from(""),
        Line::from(
            "                                                                                 ",
        ),
        Line::from(" F3=Exit   F5=Refresh   F10=Help   F12=Back"),
    ];
    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(lines)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn handle_key(code: KeyCode, state: &mut AppState) -> bool {
    if let AgentMode::Managed(_) = state.mode {
        match code {
            KeyCode::Esc | KeyCode::F(3) | KeyCode::F(12) | KeyCode::Char('q') => {
                state.should_exit = true
            }
            KeyCode::F(5) => return true,
            _ => {}
        }
        return false;
    }
    match code {
        KeyCode::Esc | KeyCode::F(12) => state.back(),
        KeyCode::F(3) | KeyCode::Char('q') => state.should_exit = true,
        KeyCode::F(5) => return true,
        KeyCode::F(9) => {
            state.message = Some("Node info refreshed".to_string());
            return true;
        }
        KeyCode::F(10) => {
            state.message =
                Some("Use number keys 1-7, F3=Exit, F5/F9=Refresh, F12=Menu".to_string());
        }
        KeyCode::Left => {
            state.screen = Screen::MainMenu;
        }
        KeyCode::Right => {
            state.screen = Screen::NodeOverview;
        }
        _ => {}
    }
    false
}

fn primary_style(state: &AppState) -> Style {
    if state.no_color {
        Style::default()
    } else {
        Style::default()
            .fg(Color::Green)
            .bg(Color::Black)
            .add_modifier(Modifier::BOLD)
    }
}

fn render_message(frame: &mut ratatui::Frame, area: Rect, message: &str, state: &AppState) {
    let area = Rect {
        x: area.x + 2,
        y: area.y + area.height.saturating_sub(3),
        width: area.width.saturating_sub(4),
        height: 3,
    };
    let mut block = Block::default().borders(Borders::ALL).title("Info");
    if !state.no_color {
        block = block.border_style(Style::default().fg(Color::Yellow));
    }
    let paragraph = Paragraph::new(message.to_string())
        .alignment(Alignment::Left)
        .style(primary_style(state))
        .block(block);
    frame.render_widget(paragraph, area);
}

fn render_placeholder(frame: &mut ratatui::Frame, area: Rect, state: &AppState, msg: &str) {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .title("Awaiting Data");
    if !state.no_color {
        block = block.border_style(Style::default().fg(Color::Yellow));
    }
    let lines = vec![
        Line::from(msg.to_string()),
        Line::from(""),
        Line::from("Ensure esnode-core daemon is running and reachable, then press F5."),
    ];
    let paragraph = Paragraph::new(lines)
        .style(primary_style(state))
        .alignment(Alignment::Left)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_managed(frame: &mut ratatui::Frame, area: Rect, state: &AppState) {
    let meta = match &state.mode {
        AgentMode::Managed(m) => Some(m),
        _ => None,
    };
    let lines = vec![
        Line::from("                     ESNODE-AGENT – MANAGED BY ESNODE-SERVER             N01"),
        Line::from(""),
        Line::from(format!(
            "   Node Mode  . . . . . . . . . . . . . . . :  {}",
            meta.map(|_| "MANAGED").unwrap_or("UNKNOWN")
        )),
        Line::from(format!(
            "   Node ID  . . . . . . . . . . . . . . . . :  {}",
            meta.and_then(|m| m.node_id.clone())
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!(
            "   Cluster ID  . . . . . . . . . . . . . .  :  {}",
            meta.and_then(|m| m.cluster_id.clone())
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(""),
        Line::from("   ESNODE-Pulse:"),
        Line::from(format!(
            "     Address . . . . . . . . . . . . . . .  :  {}",
            meta.and_then(|m| m.server.clone())
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!(
            "     Last contact (UTC) . . . . . . . . . . :  {}",
            meta.and_then(|m| m.last_contact_unix_ms)
                .map(|ms| format!("{}", ms))
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Line::from(format!(
            "     Connection state  . . . . . . . . . .  :  {}",
            meta.map(|m| m.state.clone())
                .unwrap_or_else(|| "DEGRADED".to_string())
        )),
        Line::from(""),
        Line::from("   Local Monitoring:"),
        Line::from("     Prometheus endpoint (/metrics)  . . . .:  ENABLED"),
        Line::from("     OTLP / JSON / file sinks  . . . . . .  :  ENABLED (per config)"),
        Line::from(""),
        Line::from("   Local control of metrics profiles, alerts, and throttling"),
        Line::from("   is disabled while this node is managed by ESNODE-Pulse."),
        Line::from(""),
        Line::from("   To change policies, please use the ESNODE-Pulse console:"),
        Line::from(""),
        Line::from("     $ esnode-pulse cli   (on the master/server host)"),
        Line::from(""),
        Line::from(""),
        Line::from(" F3=Exit   F5=Refresh   F12=Cancel"),
    ];

    let mut block = Block::default().borders(Borders::ALL);
    if !state.no_color {
        block = block.border_style(primary_style(state));
    }
    let paragraph = Paragraph::new(lines)
        .style(primary_style(state))
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn build_gpu_table(status: Option<&StatusSnapshot>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(
            " GPU  User  Util%  VRAM Used / Total      Power(W)  Temp°C  Throt%  ECC  Notes",
        ),
        Line::from(
            " ---- ----- -----  --------------------- --------- ------- ------- ----  -----",
        ),
    ];

    match status {
        Some(status) if !status.gpus.is_empty() => {
            for (idx, gpu) in status.gpus.iter().enumerate() {
                lines.push(Line::from(format!(
                    " {idx:<4}{user:<6}{util:<6}{mem:<23}{power:<10}{temp:<8}{throt:<8}{ecc:<5}{notes}",
                    user = gpu_owner(gpu),
                    util = gpu
                        .util_percent
                        .map(|v| format!("{v:>5.1}"))
                        .unwrap_or_else(|| "  n/a".to_string()),
                    mem = format!(
                        "{} / {}",
                        format_bytes(gpu.memory_used_bytes),
                        format_bytes(gpu.memory_total_bytes)
                    ),
                    power = gpu
                        .power_watts
                        .map(|v| format!("{v:<9.0}"))
                        .unwrap_or_else(|| "n/a      ".to_string()),
                    temp = gpu
                        .temperature_celsius
                        .map(|v| format!("{v:<7.0}"))
                        .unwrap_or_else(|| "n/a    ".to_string()),
                    throt = format!(
                        "{:.1}",
                        if gpu.power_throttle || gpu.thermal_throttle {
                            3.0
                        } else {
                            0.0
                        }
                    ),
                    ecc = 0,
                    notes = if gpu.thermal_throttle {
                        "HOT"
                    } else if gpu.power_throttle {
                        "THROTTLING"
                    } else {
                        "OK"
                    }
                )));
            }
        }
        Some(_) => {
            lines.push(Line::from(
                "   GPU hardware not present or not supported on this node.",
            ));
        }
        None => {
            lines.push(Line::from("   no GPU data available (agent not reachable)"));
        }
    }

    lines.push(Line::from(""));
    let node_power = status
        .and_then(|s| s.node_power_watts)
        .map(|v| format!("{:.1} kW", v / 1000.0))
        .unwrap_or_else(|| "n/a".to_string());
    lines.push(Line::from(format!(
        " Node Power: {node_power}   Tokens/Watt (last 5m): n/a    Energy/J (last 24h):  n/a",
    )));
    lines
}

fn format_bytes(value: Option<f64>) -> String {
    match value {
        Some(v) if v > 0.0 => format!("{:.0} GiB", v / 1024.0 / 1024.0 / 1024.0),
        _ => "n/a".to_string(),
    }
}

fn human_bytes(v: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let f = v as f64;
    if f >= TB {
        format!("{:.1} TiB", f / TB)
    } else if f >= GB {
        format!("{:.1} GiB", f / GB)
    } else if f >= MB {
        format!("{:.1} MiB", f / MB)
    } else if f >= KB {
        format!("{:.0} KiB", f / KB)
    } else {
        format!("{v} B")
    }
}

fn format_duration(secs: u64) -> String {
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3600;
    let minutes = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn gpu_owner(gpu: &GpuStatus) -> String {
    gpu.fan_percent
        .map(|v| format!("{v:>5.1}"))
        .unwrap_or_else(|| "svc".to_string())
}

#[derive(Default)]
struct NodeSummary {
    node_name: String,
    region: String,
    uptime: String,
    cores: String,
    load_1: String,
    load_5: String,
    load_15: String,
    cpu_util: String,
    mem_total: String,
    mem_used: String,
    mem_free: String,
    swap_used: String,
    disk_used: String,
    disk_latency: String,
    net_rx: String,
    net_tx: String,
    net_drop: String,
    node_power: String,
    node_limit: String,
    spikes: String,
    therm_inlet: String,
    therm_exhaust: String,
    therm_hotspot: String,
    gpu_count: usize,
    total_vram: String,
    avg_gpu_util: String,
    avg_gpu_power: String,
    tokens_per_watt: String,
    tokens_per_joule: String,
}

impl NodeSummary {
    fn from_status(state: &AppState) -> Self {
        let mut summary = NodeSummary {
            node_name: "gpu-node-01".to_string(),
            region: "local".to_string(),
            uptime: "n/a".to_string(),
            cores: "n/a".to_string(),
            load_1: "n/a".to_string(),
            load_5: "n/a".to_string(),
            load_15: "n/a".to_string(),
            cpu_util: "n/a".to_string(),
            mem_total: "n/a".to_string(),
            mem_used: "n/a".to_string(),
            mem_free: "n/a".to_string(),
            swap_used: "n/a".to_string(),
            disk_used: "n/a".to_string(),
            disk_latency: "n/a".to_string(),
            net_rx: "n/a".to_string(),
            net_tx: "n/a".to_string(),
            net_drop: "0".to_string(),
            node_power: "n/a".to_string(),
            node_limit: "n/a".to_string(),
            spikes: "n/a".to_string(),
            therm_inlet: "n/a".to_string(),
            therm_exhaust: "n/a".to_string(),
            therm_hotspot: "n/a".to_string(),
            gpu_count: 0,
            total_vram: "0 GiB".to_string(),
            avg_gpu_util: "n/a".to_string(),
            avg_gpu_power: "n/a".to_string(),
            tokens_per_watt: "n/a".to_string(),
            tokens_per_joule: "n/a".to_string(),
        };

        if let Some(status) = state.last_status.as_ref() {
            summary.load_1 = format!("{:.1}", status.load_avg_1m);
            if let Some(l5) = status.load_avg_5m {
                summary.load_5 = format!("{:.1}", l5);
            }
            if let Some(l15) = status.load_avg_15m {
                summary.load_15 = format!("{:.1}", l15);
            }
            if let Some(cores) = status.cpu_cores {
                summary.cores = format!("{cores}");
            }
            if let Some(util) = status.cpu_util_percent {
                summary.cpu_util = format!("{util:.0} %");
            }
            if let Some(uptime) = status.uptime_seconds {
                summary.uptime = format_duration(uptime);
            }
            if let (Some(total), Some(used), Some(free)) = (
                status.mem_total_bytes,
                status.mem_used_bytes,
                status.mem_free_bytes,
            ) {
                summary.mem_total = human_bytes(total);
                summary.mem_used = human_bytes(used);
                summary.mem_free = human_bytes(free);
            }
            if let Some(swap) = status.swap_used_bytes {
                summary.swap_used = human_bytes(swap);
            }
            if let (Some(total), Some(used)) =
                (status.disk_root_total_bytes, status.disk_root_used_bytes)
            {
                summary.disk_used = format!("{} / {}", human_bytes(used), human_bytes(total));
            }
            if let Some(io_ms) = status.disk_root_io_time_ms {
                summary.disk_latency = format!("{io_ms} ms");
            }
            if let Some(nic) = status.primary_nic.clone() {
                let rx = status
                    .net_rx_bytes_per_sec
                    .map(|b| format!("{}/s", human_bytes(b as u64)))
                    .unwrap_or_else(|| "n/a".to_string());
                let tx = status
                    .net_tx_bytes_per_sec
                    .map(|b| format!("{}/s", human_bytes(b as u64)))
                    .unwrap_or_else(|| "n/a".to_string());
                let drops = status
                    .net_drops_per_sec
                    .map(|d| format!("{:.1}", d))
                    .unwrap_or_else(|| "0".to_string());
                summary.net_rx = format!("{rx} ({nic})");
                summary.net_tx = tx;
                summary.net_drop = drops;
            }
            if let Some(power) = status.node_power_watts {
                summary.node_power = format!("{:.1} W", power);
                summary.tokens_per_joule = format!("{:.1}", power / 10.0);
            } else {
                let cpu_pkg_avg: Option<f64> = {
                    let vals: Vec<f64> = status
                        .cpu_package_power_watts
                        .iter()
                        .map(|p| p.watts)
                        .collect();
                    if vals.is_empty() {
                        None
                    } else {
                        Some(vals.iter().sum::<f64>() / (vals.len() as f64))
                    }
                };
                let gpu_avg: Option<f64> = {
                    if status.gpus.is_empty() {
                        None
                    } else {
                        Some(
                            status
                                .gpus
                                .iter()
                                .filter_map(|g| g.power_watts)
                                .sum::<f64>()
                                / (status.gpus.len() as f64),
                        )
                    }
                };
                let approx = match (cpu_pkg_avg, gpu_avg) {
                    (Some(c), Some(g)) => Some(c + g),
                    (Some(c), None) => Some(c),
                    (None, Some(g)) => Some(g),
                    _ => None,
                };
                if let Some(v) = approx {
                    summary.node_power = format!("~{:.1} W", v);
                }
            }
            if !status.cpu_temperatures.is_empty() {
                let mut inlet = None;
                let mut exhaust = None;
                let mut hotspot = None;
                for t in status.cpu_temperatures.iter() {
                    let name = t.sensor.to_lowercase();
                    if inlet.is_none() && (name.contains("inlet") || name.contains("ambient")) {
                        inlet = Some(t.celsius);
                    }
                    if exhaust.is_none() && name.contains("exhaust") {
                        exhaust = Some(t.celsius);
                    }
                    hotspot = Some(match hotspot {
                        Some(h) if h >= t.celsius => h,
                        _ => t.celsius,
                    });
                }
                if let Some(v) = inlet {
                    summary.therm_inlet = format!("{:.0} C", v);
                }
                if let Some(v) = exhaust {
                    summary.therm_exhaust = format!("{:.0} C", v);
                }
                if let Some(v) = hotspot {
                    summary.therm_hotspot = format!("{:.0} C", v);
                }
            }
            if !status.gpus.is_empty() {
                summary.gpu_count = status.gpus.len();
                let total_vram_bytes: f64 = status
                    .gpus
                    .iter()
                    .filter_map(|g| g.memory_total_bytes)
                    .sum();
                if total_vram_bytes > 0.0 {
                    summary.total_vram =
                        format!("{:.0} GiB", total_vram_bytes / 1024.0 / 1024.0 / 1024.0);
                }
                let avg_util: f64 = status
                    .gpus
                    .iter()
                    .filter_map(|g| g.util_percent)
                    .sum::<f64>()
                    / (status.gpus.len() as f64);
                if avg_util > 0.0 {
                    summary.avg_gpu_util = format!("{avg_util:.0} %");
                }
                let avg_power: f64 = status
                    .gpus
                    .iter()
                    .filter_map(|g| g.power_watts)
                    .sum::<f64>()
                    / (status.gpus.len() as f64);
                if avg_power > 0.0 {
                    summary.avg_gpu_power = format!("{avg_power:.0} W/GPU");
                }
                summary.tokens_per_watt = "n/a".to_string();
            }
            if let Some(limit) = state.config.node_power_envelope_watts {
                summary.node_limit = format!("{limit:.0} W");
            }
        }

        summary
    }
}

#[derive(Default)]
struct MetricToggleState {
    host: char,
    gpu_core: char,
    gpu_power: char,
    mcp: char,
    app: char,
    rack: char,
}

impl MetricToggleState {
    fn from_status(status: Option<&StatusSnapshot>) -> Self {
        let mut toggles = MetricToggleState {
            host: 'Y',
            gpu_core: 'Y',
            gpu_power: 'Y',
            mcp: 'N',
            app: 'N',
            rack: 'N',
        };
        if status.is_none() {
            toggles.host = 'N';
            toggles.gpu_core = 'N';
            toggles.gpu_power = 'N';
        }
        toggles
    }
}
