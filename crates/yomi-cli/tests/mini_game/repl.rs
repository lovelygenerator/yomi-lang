//! REPL (spec/host/07-repl.md)

use super::*;

#[test]
fn repl_loads_mini_game_and_uses_variables_types_and_call() {
    let dir = scratch("repl");
    let main = main_yomi();
    let o = yomi(
        &dir,
        &["--lang", "en", "repl", &main, "--seed", SEED],
        "turns\nshop.money\n:type chara.mood_of(7)\nchara.mood_of(7)\nlet ok = call shop.buy(\"携帯食\")\n\nok\nshop.money\ncall greet_elder()\n\n\nchara.trust\n:vars\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stderr, "", "警告も診断も出ない");
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(
        &lines[..4],
        ["5", "1200", "chara.Mood", "Mood.Confident"],
        "{}",
        o.stdout
    );
    // The result of the purchase (`ok`, the money), and the bodies and cues inside the `call`ed label
    assert!(o.stdout.contains("[audio] se coin\n"), "{}", o.stdout);
    assert!(o.stdout.contains("ok = true\ntrue\n950\n"), "{}", o.stdout);
    assert!(
        o.stdout.contains("[image] show elder = elder_house"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("村の長老「おお、ゆうと殿とリオンか。よい組み合わせじゃ」\n")
    );
    // Variables changed by `call` are visible in the REPL. There are multiple modules, so they are shown as `module.name`
    assert!(o.stdout.contains("\n1\nchara.trust = 1\n"), "{}", o.stdout);
    assert!(o.stdout.contains("shop.money = 950\n"), "{}", o.stdout);
    assert!(
        o.stdout.contains("-- REPL variables --\nok = true\n"),
        "{}",
        o.stdout
    );
}

#[test]
fn run_repl_apply_affects_rest_of_play() {
    let dir = scratch("repl_apply");
    let main = main_yomi();
    let o = yomi(
        &dir,
        &["--lang", "en", "run", &main, "--seed", SEED],
        ":repl\nturns\nturns = 1\n:apply\n:exit\n:vars\n",
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.contains("(entering the REPL"), "{}", o.stdout);
    assert!(o.stdout.contains("5\nmain.turns = 1\n"), "{}", o.stdout);
    // It was applied, so the first stop point is shown again and also appears in `:vars`
    assert_eq!(
        o.stdout
            .matches("昼下がりの村の門。相棒のリオンが、地図を広げて待っていた。\n")
            .count(),
        2,
        "{}",
        o.stdout
    );
    assert!(
        o.stdout.contains("main.turns = 1\nmain.promised = false\n"),
        "{}",
        o.stdout
    );
    // One visit left, so after the first choice it heads to the ending (if `turns` were still 5, it would still be in the
    // village)
    assert!(
        o.stdout
            .contains("（日暮れまでに、あと 1 か所は回れそうだ）"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.ends_with("(end)\n"), "{}", o.stdout);
}
