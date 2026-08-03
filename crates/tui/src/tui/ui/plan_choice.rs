//! Plan-mode choice parsing.
//!
//! Pure helpers that map user input (a 1-4 option or typed text) to the
//! [`PlanChoice`] the event loop acts on. Kept free of `App`/engine state so it
//! can live independently of the godfile's event-loop machinery.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanChoice {
    AcceptAgent,
    AcceptYolo,
    RevisePlan,
    ExitPlan,
}

pub(crate) fn plan_next_step_prompt() -> String {
    [
        "Action required: choose the next step for this plan.",
        "  1) Accept + implement in Agent mode",
        "  2) Accept + implement in YOLO mode",
        "  3) Revise the plan / ask follow-ups",
        "  4) Return to Agent mode without implementing",
        "",
        "Use the plan confirmation popup, or type 1-4 and press Enter.",
    ]
    .join("\n")
}

pub(crate) fn plan_choice_from_option(option: usize) -> Option<PlanChoice> {
    match option {
        1 => Some(PlanChoice::AcceptAgent),
        2 => Some(PlanChoice::AcceptYolo),
        3 => Some(PlanChoice::RevisePlan),
        4 => Some(PlanChoice::ExitPlan),
        _ => None,
    }
}

pub(crate) fn parse_plan_choice(input: &str) -> Option<PlanChoice> {
    // Once the modal is dismissed, only the advertised 1-4 fallback remains active.
    // Letter shortcuts stay modal-only so normal messages like "yolo" are not captured.
    match input.trim() {
        "1" => Some(PlanChoice::AcceptAgent),
        "2" => Some(PlanChoice::AcceptYolo),
        "3" => Some(PlanChoice::RevisePlan),
        "4" => Some(PlanChoice::ExitPlan),
        _ => None,
    }
}
