//! Script debugging while playing (Unity's Console tools): expressions evaluated in the running
//! game (`$sel` = the selected object, `$scripts` = its behaviours), Error Pause, `Debug.break()`,
//! and hot reload of changed scripts (the running objects keep their state).

use goat_script::ScriptDebug;
use serde_json::{json, Value};

use crate::core::EditorCore;

#[derive(Default)]
pub struct DebugState {
    next_id: u32,
    /// Answers not yet given to the UI: (id, text, ok).
    answers: Vec<(u32, String, bool)>,
    /// Pause on script errors (the Console's Error Pause).
    pub error_pause: bool,
    /// Why the game paused itself (Debug.break / an error), for the UI once.
    pause_reason: Option<String>,
}

impl EditorCore {
    /// Queues a console expression for the running game (answered after the next frame).
    pub fn console_eval(&mut self, code: &str) -> Option<u32> {
        let selected = self.selection.first().copied();
        let play = self.play.as_mut()?;
        self.debug.next_id += 1;
        let id = self.debug.next_id;
        play.resources.get_or_default::<ScriptDebug>().evals.push((id, code.to_string(), selected));
        // paused: the request still needs a frame (a zero-time one)
        if self.paused {
            self.step_zero = true;
        }
        Some(id)
    }

    /// `[{ id, text, ok }]` of the expressions answered since the last call.
    pub fn console_results(&mut self) -> Value {
        Value::Array(self.debug.answers.drain(..).map(|(id, text, ok)| json!({ "id": id, "text": text, "ok": ok })).collect())
    }

    /// Why the game paused itself since the last call ("" = it didn't).
    pub fn take_pause_reason(&mut self) -> String {
        self.debug.pause_reason.take().unwrap_or_default()
    }

    /// A script changed while playing: its running objects get the new code.
    pub fn hot_reload(&mut self, key: &str) {
        let Some(play) = self.play.as_mut() else { return };
        // C#: goat-cs rewrote X.cs.js, the instances run "X.cs"
        let src = key.strip_suffix(".js").filter(|k| k.ends_with(".cs")).unwrap_or(key);
        play.resources.get_or_default::<ScriptDebug>().reloads.push(src.to_string());
    }

    /// After a play frame: answers, Debug.break, Error Pause.
    pub(crate) fn after_play_frame(&mut self) {
        let Some(play) = self.play.as_mut() else { return };
        let Some(d) = play.resources.get_mut::<ScriptDebug>() else { return };
        for (id, r) in d.results.drain(..) {
            match r {
                Ok(v) => self.debug.answers.push((id, v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()), true)),
                Err(e) => self.debug.answers.push((id, e, false)),
            }
        }
        let errors = std::mem::take(&mut d.errors);
        if let Some(msg) = d.break_requested.take() {
            self.paused = true;
            self.debug.pause_reason = Some(if msg.is_empty() { "Debug.break()".into() } else { format!("Debug.break: {msg}") });
        } else if self.debug.error_pause && !errors.is_empty() {
            self.paused = true;
            self.debug.pause_reason = Some(format!("Error Pause: {}", errors[0]));
        }
    }
}
