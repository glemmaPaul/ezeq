use nih_plug::prelude::Params;
use nih_plug_egui::EguiState;
use std::sync::Arc;

/// Host-visible and persisted plugin state.
///
/// Keep parameter IDs stable after release: DAWs use them to restore sessions
/// and automation. EQ band parameters will be added here once the DSP model is
/// defined.
#[derive(Params)]
pub struct EzeqParams {
    #[persist = "editor-state"]
    pub(crate) editor_state: Arc<EguiState>,
}

impl Default for EzeqParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(900, 560),
        }
    }
}
