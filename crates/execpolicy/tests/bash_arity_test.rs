use mimofan_execpolicy::bash_arity::*;
use mimofan_execpolicy::*;


fn dict() -> BashArityDict {
    BashArityDict::new()
}

// ── classify ─────────────────────────────────────────────────────────────

#[test]
fn classify_git_status_bare() {
    assert_eq!(dict().classify(&["git", "status"]), "git status");
}

#[test]
fn classify_git_status_with_short_flag() {
    assert_eq!(dict().classify(&["git", "status", "-s"]), "git status");
}

#[test]
fn classify_git_status_with_long_flag() {
    assert_eq!(
        dict().classify(&["git", "status", "--porcelain"]),
        "git status"
    );
}

#[test]
fn classify_git_push() {
    assert_eq!(
        dict().classify(&["git", "push", "origin", "main"]),
        "git push"
    );
}

#[test]
fn classify_git_push_force() {
    assert_eq!(dict().classify(&["git", "push", "--force"]), "git push");
}

#[test]
fn classify_npm_run_dev_arity_3() {
    assert_eq!(dict().classify(&["npm", "run", "dev"]), "npm run dev");
}

#[test]
fn classify_npm_install() {
    assert_eq!(dict().classify(&["npm", "install"]), "npm install");
}

#[test]
fn classify_cargo_check_with_flag() {
    assert_eq!(
        dict().classify(&["cargo", "check", "--workspace"]),
        "cargo check"
    );
}

#[test]
fn classify_docker_compose_up_arity_3() {
    assert_eq!(
        dict().classify(&["docker", "compose", "up"]),
        "docker compose up"
    );
}

#[test]
fn classify_kubectl_get_pods_arity_3() {
    assert_eq!(
        dict().classify(&["kubectl", "get", "pods"]),
        "kubectl get pods"
    );
}

#[test]
fn classify_go_mod_tidy_arity_3() {
    assert_eq!(dict().classify(&["go", "mod", "tidy"]), "go mod tidy");
}

#[test]
fn classify_make_no_subcommand() {
    assert_eq!(dict().classify(&["make", "all"]), "make");
}

#[test]
fn classify_aws_s3_arity_3() {
    assert_eq!(dict().classify(&["aws", "s3", "ls"]), "aws s3 ls");
}

#[test]
fn classify_terraform_plan() {
    assert_eq!(
        dict().classify(&["terraform", "plan", "-out=tfplan"]),
        "terraform plan"
    );
}

#[test]
fn classify_unknown_falls_back_to_base() {
    assert_eq!(dict().classify(&["ls", "-la"]), "ls");
}

#[test]
fn classify_empty_returns_empty() {
    assert_eq!(dict().classify(&[]), "");
}

// ── allow_rule_matches ────────────────────────────────────────────────────

#[test]
fn allow_rule_git_status_matches_with_flag() {
    assert!(dict().allow_rule_matches("git status", "git status -s"));
}

#[test]
fn allow_rule_git_status_matches_porcelain() {
    assert!(dict().allow_rule_matches("git status", "git status --porcelain"));
}

#[test]
fn allow_rule_git_status_does_not_match_push() {
    assert!(!dict().allow_rule_matches("git status", "git push origin main"));
}

#[test]
fn allow_rule_git_status_does_not_match_checkout() {
    assert!(!dict().allow_rule_matches("git status", "git checkout main"));
}

#[test]
fn allow_rule_npm_run_matches_dev() {
    assert!(dict().allow_rule_matches("npm run dev", "npm run dev"));
}

#[test]
fn allow_rule_npm_run_dev_does_not_match_build() {
    assert!(!dict().allow_rule_matches("npm run dev", "npm run build"));
}

#[test]
fn allow_rule_cargo_check_matches_with_flags() {
    assert!(dict().allow_rule_matches("cargo check", "cargo check --workspace"));
}

#[test]
fn allow_rule_exact_match_still_works() {
    // A pattern not in the arity table falls back to exact/prefix match.
    assert!(dict().allow_rule_matches("ls", "ls -la"));
}

#[test]
fn allow_rule_make_matches_with_target() {
    assert!(dict().allow_rule_matches("make", "make all"));
    assert!(dict().allow_rule_matches("make", "make clean"));
}

#[test]
fn allow_rule_aws_s3_ls() {
    assert!(dict().allow_rule_matches("aws s3 ls", "aws s3 ls"));
    // "aws s3 cp" should not match "aws s3 ls"
    assert!(!dict().allow_rule_matches("aws s3 ls", "aws s3 cp src dst"));
}

// ── coverage count ────────────────────────────────────────────────────────

#[test]
fn dict_covers_at_least_30_commands() {
    // The issue requires 30+ common commands covered.
    assert!(
        BashArityDict::new().len() >= 30,
        "expected at least 30 entries, got {}",
        BashArityDict::new().len()
    );
}
