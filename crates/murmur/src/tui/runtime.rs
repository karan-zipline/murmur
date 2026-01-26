use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::event::{Event as CEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, terminal};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use super::client::{EventStream, TuiClient};
use super::core::{reduce, Action, Effect, Model, Msg};
use super::view;

struct TerminalGuard {
    stdout: Stdout,
}

impl TerminalGuard {
    fn enter() -> Result<(Self, Terminal<CrosstermBackend<Stdout>>)> {
        enable_raw_mode().context("enable raw mode")?;

        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnterAlternateScreen,
            EnableBracketedPaste,
            terminal::Clear(terminal::ClearType::All)
        )
        .context("enter alt screen")?;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).context("create terminal")?;

        Ok((
            Self {
                stdout: io::stdout(),
            },
            terminal,
        ))
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(self.stdout, DisableBracketedPaste, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

pub async fn run(client: Arc<dyn TuiClient>) -> Result<()> {
    let shutdown = Arc::new(AtomicBool::new(false));

    let (_guard, mut terminal) = TerminalGuard::enter()?;

    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<Msg>();
    // Channel for async effect results
    let (effect_tx, mut effect_rx) = mpsc::unbounded_channel::<Msg>();

    spawn_input_pump(msg_tx.clone(), shutdown.clone());
    spawn_tick_pump(msg_tx.clone(), shutdown.clone());

    let mut model = Model::new();
    let size = terminal.size().context("terminal size")?;
    (model, _) = reduce(
        model,
        Msg::Resize {
            width: size.width,
            height: size.height,
        },
    );
    (model, _) = reduce(
        model,
        Msg::Tick {
            now_ms: unix_epoch_ms(),
        },
    );

    let mut stream: Option<EventStream> = None;

    let (next, effects) = reduce(model, Msg::Init);
    let (mut model, quit) =
        apply_effects(next, effects, client.clone(), &mut stream, &effect_tx).await?;
    if quit {
        shutdown.store(true, Ordering::Relaxed);
        return Ok(());
    }

    terminal.draw(|f| view::draw(f, &model)).context("draw")?;

    loop {
        tokio::select! {
            msg = msg_rx.recv() => {
                let Some(msg) = msg else { break };
                let (next, effects) = reduce(model, msg);
                let (next, quit) = apply_effects(next, effects, client.clone(), &mut stream, &effect_tx).await?;
                model = next;
                if model.dirty {
                    terminal.draw(|f| view::draw(f, &model)).context("draw")?;
                    model.dirty = false;
                }
                if quit { break; }
            }
            // Handle async effect results
            effect_result = effect_rx.recv() => {
                let Some(msg) = effect_result else { continue };
                let (next, effects) = reduce(model, msg);
                let (next, quit) = apply_effects(next, effects, client.clone(), &mut stream, &effect_tx).await?;
                model = next;
                if model.dirty {
                    terminal.draw(|f| view::draw(f, &model)).context("draw")?;
                    model.dirty = false;
                }
                if quit { break; }
            }
            evt = async {
                match stream.as_mut() {
                    Some(s) => s.recv().await,
                    None => None,
                }
            }, if stream.is_some() => {
                match evt {
                    Some(Ok(event)) => {
                        let msg = match event.r#type.as_str() {
                            murmur_protocol::EVT_HEARTBEAT => {
                                match serde_json::from_value::<murmur_protocol::HeartbeatEvent>(event.payload) {
                                    Ok(hb) => Some(Msg::Tick { now_ms: hb.now_ms }),
                                    Err(_) => None,
                                }
                            }
                            murmur_protocol::EVT_AGENT_CHAT => {
                                match serde_json::from_value::<murmur_protocol::AgentChatEvent>(event.payload) {
                                    Ok(chat) => Some(Msg::AgentChatReceived(chat)),
                                    Err(_) => None,
                                }
                            }
                            murmur_protocol::EVT_AGENT_CREATED => {
                                match serde_json::from_value::<murmur_protocol::AgentCreatedEvent>(event.payload) {
                                    Ok(evt) => Some(Msg::AgentCreated(evt)),
                                    Err(_) => None,
                                }
                            }
                            murmur_protocol::EVT_AGENT_DELETED => {
                                match serde_json::from_value::<murmur_protocol::AgentDeletedEvent>(event.payload) {
                                    Ok(evt) => Some(Msg::AgentDeleted(evt)),
                                    Err(_) => None,
                                }
                            }
                            murmur_protocol::EVT_PERMISSION_REQUEST => {
                                match serde_json::from_value::<murmur_protocol::PermissionRequest>(event.payload) {
                                    Ok(req) => Some(Msg::PermissionRequested(req)),
                                    Err(_) => None,
                                }
                            }
                            murmur_protocol::EVT_USER_QUESTION => {
                                match serde_json::from_value::<murmur_protocol::UserQuestion>(event.payload) {
                                    Ok(req) => Some(Msg::QuestionRequested(req)),
                                    Err(_) => None,
                                }
                            }
                            _ => None,
                        };

                        if let Some(msg) = msg {
                            let (next, effects) = reduce(model, msg);
                            let (next, quit) = apply_effects(next, effects, client.clone(), &mut stream, &effect_tx).await?;
                            model = next;
                            if model.dirty {
                                terminal.draw(|f| view::draw(f, &model)).context("draw")?;
                                model.dirty = false;
                            }
                            if quit { break; }
                        }
                    }
                    Some(Err(err)) => {
                        stream = None;
                        let (next, effects) = reduce(model, Msg::StreamDisconnected { reason: err.to_string() });
                        let (next, _quit) = apply_effects(next, effects, client.clone(), &mut stream, &effect_tx).await?;
                        model = next;
                        if model.dirty {
                            terminal.draw(|f| view::draw(f, &model)).context("draw")?;
                            model.dirty = false;
                        }
                    }
                    None => {
                        stream = None;
                        let (next, _effects) = reduce(model, Msg::StreamDisconnected { reason: "event stream closed".to_owned() });
                        model = next;
                        if model.dirty {
                            terminal.draw(|f| view::draw(f, &model)).context("draw")?;
                            model.dirty = false;
                        }
                    }
                }
            }
        }
    }

    shutdown.store(true, Ordering::Relaxed);
    Ok(())
}

fn spawn_input_pump(tx: mpsc::UnboundedSender<Msg>, shutdown: Arc<AtomicBool>) {
    tokio::task::spawn_blocking(move || {
        while !shutdown.load(Ordering::Relaxed) {
            let ready = match crossterm::event::poll(Duration::from_millis(50)) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !ready {
                continue;
            }

            let evt = match crossterm::event::read() {
                Ok(evt) => evt,
                Err(_) => continue,
            };

            let msg = match evt {
                CEvent::Key(key) => map_key(key).map(Msg::Action),
                CEvent::Paste(text) => Some(Msg::Paste(text)),
                CEvent::Resize(w, h) => Some(Msg::Resize {
                    width: w,
                    height: h,
                }),
                _ => None,
            };

            if let Some(msg) = msg {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        }
    });
}

fn spawn_tick_pump(tx: mpsc::UnboundedSender<Msg>, shutdown: Arc<AtomicBool>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        loop {
            interval.tick().await;
            if shutdown.load(Ordering::Relaxed) {
                return;
            }
            let _ = tx.send(Msg::Tick {
                now_ms: unix_epoch_ms(),
            });
        }
    });
}

fn unix_epoch_ms() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis().min(u64::MAX as u128) as u64
}

fn map_key(key: KeyEvent) -> Option<Action> {
    if !matches!(key.kind, KeyEventKind::Press) {
        return None;
    }

    if matches!(key.code, KeyCode::Char('c')) && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::Quit);
    }

    match key.code {
        KeyCode::Tab => Some(Action::Tab),
        KeyCode::Esc => Some(Action::Cancel),
        KeyCode::Up => Some(Action::MoveUp),
        KeyCode::Down => Some(Action::MoveDown),
        KeyCode::Home => Some(Action::GoTop),
        KeyCode::End => Some(Action::GoBottom),
        KeyCode::PageUp => Some(Action::PageUp),
        KeyCode::PageDown => Some(Action::PageDown),
        KeyCode::Backspace => Some(Action::Backspace),
        KeyCode::Enter => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                Some(Action::ShiftEnter)
            } else {
                Some(Action::Enter)
            }
        }
        KeyCode::Char(c) => {
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            {
                None
            } else {
                Some(Action::Char(c))
            }
        }
        _ => None,
    }
}

/// Apply effects, spawning background tasks for network operations.
/// Non-blocking effects are spawned immediately and results are sent through effect_tx.
/// Stream-related effects must be processed synchronously since they manage stream state.
async fn apply_effects(
    mut model: Model,
    effects: Vec<Effect>,
    client: Arc<dyn TuiClient>,
    stream: &mut Option<EventStream>,
    effect_tx: &mpsc::UnboundedSender<Msg>,
) -> Result<(Model, bool)> {
    let mut quit = false;
    let mut sync_effects = Vec::new();

    for effect in effects {
        match effect {
            Effect::Quit => {
                quit = true;
            }
            // Spawn background tasks for fetch operations
            Effect::FetchAgentList => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .agent_list()
                        .await
                        .map(|resp| resp.agents)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::AgentListLoaded(result));
                });
            }
            Effect::FetchStats { project } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client.stats(project).await.map_err(|err| err.to_string());
                    let _ = tx.send(Msg::StatsLoaded(result));
                });
            }
            Effect::FetchCommitList { project, limit } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .commit_list(project, Some(limit))
                        .await
                        .map(|resp| resp.commits)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::CommitListLoaded(result));
                });
            }
            Effect::FetchProjectList => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .project_list()
                        .await
                        .map(|resp| resp.projects)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::ProjectListLoaded(result));
                });
            }
            Effect::FetchAgentChatHistory { agent_id, limit } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .agent_chat_history(agent_id.clone(), Some(limit))
                        .await
                        .map(|resp| resp.messages)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::AgentChatHistoryLoaded { agent_id, result });
                });
            }
            Effect::FetchPermissionList { project } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .permission_list(project)
                        .await
                        .map(|resp| resp.requests)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::PermissionListLoaded(result));
                });
            }
            Effect::FetchQuestionList { project } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .question_list(project)
                        .await
                        .map(|resp| resp.requests)
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::QuestionListLoaded(result));
                });
            }
            // These effects need synchronous processing (user is waiting for response)
            Effect::SendAgentMessage { agent_id, message } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .agent_send_message(agent_id.clone(), message)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::AgentSendMessageFinished { agent_id, result });
                });
            }
            Effect::AbortAgent { agent_id } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .agent_abort(agent_id.clone())
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::AbortFinished { agent_id, result });
                });
            }
            Effect::StopPlan { plan_id } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .plan_stop(plan_id.clone())
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::PlanStopFinished { plan_id, result });
                });
            }
            Effect::RespondPermission { id, behavior } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .permission_respond(id.clone(), behavior)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::PermissionRespondFinished { id, result });
                });
            }
            Effect::RespondQuestion { id, answers } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .question_respond(id.clone(), answers)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::QuestionRespondFinished { id, result });
                });
            }
            Effect::StartPlan { project, prompt } => {
                let client = client.clone();
                let tx = effect_tx.clone();
                tokio::spawn(async move {
                    let result = client
                        .plan_start(project, prompt)
                        .await
                        .map_err(|err| err.to_string());
                    let _ = tx.send(Msg::PlanStartFinished(result));
                });
            }
            // Stream effects must be synchronous since they manage the stream state
            Effect::AttachStream { .. } | Effect::ReconnectStream => {
                sync_effects.push(effect);
            }
        }
    }

    // Process stream effects synchronously
    for effect in sync_effects {
        match effect {
            Effect::AttachStream { projects } => {
                *stream = None;
                match client.stream_events(projects).await {
                    Ok(s) => {
                        *stream = Some(s);
                        let (next, more) = reduce(model, Msg::StreamConnected);
                        // Recursively apply any new effects
                        let (next, q) =
                            Box::pin(apply_effects(next, more, client.clone(), stream, effect_tx))
                                .await?;
                        model = next;
                        quit = quit || q;
                    }
                    Err(err) => {
                        let (next, more) = reduce(
                            model,
                            Msg::StreamDisconnected {
                                reason: err.to_string(),
                            },
                        );
                        let (next, q) =
                            Box::pin(apply_effects(next, more, client.clone(), stream, effect_tx))
                                .await?;
                        model = next;
                        quit = quit || q;
                    }
                }
            }
            Effect::ReconnectStream => {
                *stream = None;
                match client.stream_events(vec![]).await {
                    Ok(s) => {
                        *stream = Some(s);
                        let (next, more) = reduce(model, Msg::StreamConnected);
                        let (next, q) =
                            Box::pin(apply_effects(next, more, client.clone(), stream, effect_tx))
                                .await?;
                        model = next;
                        quit = quit || q;
                    }
                    Err(err) => {
                        let (next, more) = reduce(
                            model,
                            Msg::StreamDisconnected {
                                reason: err.to_string(),
                            },
                        );
                        let (next, q) =
                            Box::pin(apply_effects(next, more, client.clone(), stream, effect_tx))
                                .await?;
                        model = next;
                        quit = quit || q;
                    }
                }
            }
            _ => {}
        }
    }

    Ok((model, quit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keymap_maps_expected_actions() {
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Char('q'))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::Tab)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::Enter)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Some(Action::ShiftEnter)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::Cancel)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
            Some(Action::Char('j'))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::MoveDown)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE)),
            Some(Action::Char('k'))
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            Some(Action::MoveUp)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            Some(Action::GoTop)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            Some(Action::GoBottom)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
            Some(Action::PageUp)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
            Some(Action::PageDown)
        );
        assert_eq!(
            map_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::Backspace)
        );
    }
}
