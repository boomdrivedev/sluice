import type { On } from "claude-code";
import type { Engine, MockClock, TestBody } from "claude-code/testing";
import { expect, mock, test } from "claude-code/testing";

import type { Flaky, Mine, Overview, Recent } from "../types";
import {
  NOW,
  overview,
  withChecks,
  withConflict,
  withHeld,
  withLinks,
  withPreviews,
  withRelease,
  withTasks,
} from "./fixture";

const ROOT = "/work/sluice/plugin-board";
const WHOLE = { isStdoutTruncated: false, isStderrTruncated: false };
const LOG = "/work/sluice/sluice.git/sluice/.logs/main/c41/cli:test.log";
/** The log of the run a flaky failure contradicts. */
const OTHER = "/work/sluice/sluice.git/sluice/.logs/main/c38/cli:test.log";

/** `cli:test` passed here, read as failed for a fail recorded earlier, whose log is `log`. */
function flaky(log?: string): Flaky {
  return {
    here: "pass",
    at: "2026-10-16T19:42:05Z",
    host: "devbox",
    other: {
      outcome: "fail",
      at: "2026-10-16T19:10:30Z",
      branch: "main",
      combination: "4f1c2aa0e1",
      host: "mac-studio",
      ...(log ? { log } : {}),
    },
  };
}

/** The fixture, then #13 blamed. */
function blamed(): Overview {
  const d = overview();
  d.branches[0]!.candidates = [];
  d.branches[0]!.submissions = d.branches[0]!.submissions.filter((s) => s.id !== 13);
  d.branches[0]!.depth = 2;
  d.branches[0]!.recent.push({
    schema: "sluice/log@1",
    at: "2026-10-16T19:50:00Z",
    event: "blamed",
    id: 13,
    combination: "e20a6f9b48",
  });
  return d;
}

/** Every command the host ran, in order. */
const ran: string[] = [];

type Answer = { exitCode: number; stdout: string; stderr: string };

/**
 * What the host's `sluice inbox` answers, run after run, each host setting
 * it afresh: `answers` in turn, then nothing owed; while `held` is set, a
 * run waits on it before it answers. The worktree each run ran in, and the
 * command line, `sluice inbox` or `sluice inbox --new`, are kept.
 */
const inbox = {
  answers: [] as Answer[],
  held: null as Promise<void> | null,
  ranIn: [] as (string | undefined)[],
  ran: [] as string[],
};

const INBOX = "sluice inbox";
const INBOX_NEW = "sluice inbox --new";

/** The host's answer to `argv`, `sluice inbox` or its `--new`, run in `cwd`, as `inbox` says. */
async function inboxRun(cwd: string | undefined, argv: string) {
  inbox.ranIn.push(cwd);
  inbox.ran.push(argv);
  const answer = inbox.answers.shift() ?? { exitCode: 0, stdout: "", stderr: "" };
  if (inbox.held) await inbox.held;
  return { value: { ...answer, ...WHOLE } };
}

/** A host's `sluice inbox` prints nothing until the test says otherwise. */
function inboxAfresh() {
  inbox.answers = [];
  inbox.held = null;
  inbox.ranIn = [];
  inbox.ran = [];
}

/** What `sluice inbox` printed for #13's blame: the notice, the wait's lines, then its key. */
const BLAME13 = [
  "[sluice] #13 failed on main: cli:test. sluice wait 13 shows why.",
  "failed · blamed for cli:test on e20a6f9",
  `  cli:test  log: ${LOG}`,
  "Confirm it once you have acted on it: sluice confirm 13:failed",
].join("\n");

/** What `sluice inbox` printed for #13's landing. */
const LANDED13 = [
  "[sluice] #13 landed on main as 5d6e7f8.",
  "passed · landed as 5d6e7f8",
  "Confirm it once you have acted on it: sluice confirm 13:landed",
].join("\n");

/** `sluice inbox`'s answer printing `text`, as it prints it. */
function printing(text: string): Answer {
  return { exitCode: 0, stdout: `${text}\n`, stderr: "" };
}

/** The fixture, with #9's blame and its failed test kept on a combination. */
function blamedWithLog(log: string, f?: Flaky): Overview {
  const d = overview();
  d.branches[0]!.candidates.push({
    id: "c40",
    submissions: [9],
    revision: "4f1c2aa0e1",
    state: "failed",
    targets: ["#check-fast"],
    executor: "sandbox",
    ran: 1,
    replayed: 0,
    failed: [{ task: "cli:test", log, ...(f ? { flaky: f } : {}) }],
  });
  return d;
}

/**
 * A host where sluice answers from `docs`, streamed by its watch in turn, and
 * the session was started with the variables `env`; a command `more` names
 * prints what it says there, before any other answer; `sluice inbox`
 * answers as `inbox` says.
 */
function host(
  on: On,
  docs: Overview[],
  seen: { prompts: string[]; toasts: string[]; status: (string | undefined)[] },
  env: Record<string, string> = {},
  more: Record<string, string> = {},
) {
  mock.env(on, env);
  inboxAfresh();
  on("session.start", ($, e) => ({ cwd: e.cwd }));
  on("command.register", ($, e) => ({ value: { command: e.name } }));
  on("process.run", ($, e) => {
    const argv = e.argv.join(" ");
    ran.push(argv);
    const answer = more[argv];
    if (answer !== undefined)
      return { value: { exitCode: 0, stdout: answer, stderr: "", ...WHOLE } };
    if (argv === INBOX || argv === INBOX_NEW) return inboxRun(e.init?.cwd, argv);
    if (argv === `tail -n 20 -- ${LOG}`)
      return { value: { exitCode: 0, stdout: "first\nlast line\n", stderr: "", ...WHOLE } };
    if (argv === "sluice status 9 --branch main --json") {
      return {
        value: {
          exitCode: 0,
          stdout: JSON.stringify({ id: 9, failed: [{ task: "cli:test", log: LOG }] }),
          stderr: "",
          ...WHOLE,
        },
      };
    }
    if (argv.startsWith("tail -n 20 -- "))
      return {
        value: { exitCode: 1, stdout: "", stderr: "tail: No such file or directory\n", ...WHOLE },
      };
    if (argv === "git remote get-url origin") {
      return {
        value: {
          exitCode: 0,
          stdout: "git@github.com:agentplot/sluice.git\n",
          stderr: "",
          ...WHOLE,
        },
      };
    }
    if (argv === "git rev-parse --show-toplevel")
      return { value: { exitCode: 0, stdout: `${ROOT}\n`, stderr: "", ...WHOLE } };
    if (argv === "sluice status 13 --branch main --json") {
      return {
        value: {
          exitCode: 0,
          stdout: JSON.stringify({ id: 13, failed: [{ task: "cli:test", log: LOG }] }),
          stderr: "",
          ...WHOLE,
        },
      };
    }
    return { value: { exitCode: 1, stdout: "", stderr: `no ${argv}`, ...WHOLE } };
  });
  on("process.spawn", async function* () {
    for (const d of docs) yield { stream: "stdout" as const, text: `${JSON.stringify(d)}\n` };
    return { value: { code: 0, signal: null } };
  });
  on("ui.status", ($, e) => {
    seen.status.push(e.text);
    return { value: undefined };
  });
  on("ui.toast", ($, e) => {
    seen.toasts.push(e.text);
    return { value: undefined };
  });
  on("ui.notify", () => ({ value: { isSent: true as const, channel: "terminal_bell" } }));
  on("prompt.submit", ($, e) => {
    seen.prompts.push(e.text);
    return { text: e.text };
  });
}

/**
 * The host's `sluice overview --json --watch`, as the test drives it: each
 * watch stays open, printing what the test hands it, until the test ends it;
 * while `failing`, one fails to start, and while `refusing`, one writes it to
 * stderr and exits 2 at once. It keeps when each watch started and ended, by
 * the mocked clock, what each ran and where, and the most that ran at one
 * time.
 */
function watches(on: On, clock: MockClock) {
  type Piece = { stream: "stdout" | "stderr"; text: string };
  type Open = {
    pieces: Piece[];
    ended: { code: number | null; signal: string | null } | null;
    wake: () => void;
  };
  let open: Open | null = null;
  const w = {
    failing: false,
    refusing: null as string | null,
    started: [] as number[],
    ended: [] as number[],
    spawned: [] as { argv: string; cwd: string | undefined }[],
    running: 0,
    most: 0,
    isOpen: () => open !== null,
    /** Prints `text` on the open watch as it is. */
    async say(text: string) {
      open!.pieces.push({ stream: "stdout", text });
      open!.wake();
      await clock.settle();
    },
    /** Prints `d` on the open watch, as one line. */
    print(d: object) {
      return w.say(`${JSON.stringify(d)}\n`);
    },
    /** Ends the open watch, as one killed ends. */
    async end() {
      open!.ended = { code: null, signal: "SIGTERM" };
      open!.wake();
      await clock.settle();
    },
    /** Ends the open watch as sluice exits `code`, having written `stderr`. */
    async exit(code: number, stderr: string) {
      if (stderr) open!.pieces.push({ stream: "stderr", text: stderr });
      open!.ended = { code, signal: null };
      open!.wake();
      await clock.settle();
    },
  };
  on("process.spawn", async function* ($, e) {
    w.started.push(clock.now());
    w.spawned.push({ argv: e.argv.join(" "), cwd: e.cwd });
    if (w.failing) throw new Error("sluice: command not found");
    w.running += 1;
    w.most = Math.max(w.most, w.running);
    const me: Open = w.refusing
      ? {
          pieces: [{ stream: "stderr", text: w.refusing }],
          ended: { code: 2, signal: null },
          wake: () => undefined,
        }
      : { pieces: [], ended: null, wake: () => undefined };
    open = me;
    try {
      for (;;) {
        const piece = me.pieces.shift();
        if (piece !== undefined) yield piece;
        else if (me.ended) return { value: me.ended };
        else await new Promise<void>((woken) => (me.wake = woken));
      }
    } finally {
      if (open === me) open = null;
      w.running -= 1;
      w.ended.push(clock.now());
    }
  });
  return w;
}

const BAND = { hasSurvey: false, isWorking: false, maxRows: 4, bodyColumns: 120 } as never;
const PANE = { title: "sluice", isFocused: false, bodyColumns: 80, placement: "dock" } as never;
const INLINE = { title: "sluice", isFocused: true, bodyColumns: 100, placement: "inline" } as never;

test("the band and the pane show the queue on every surface", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  host(on, [overview()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.status).toContain("sluice: 3 submissions · yours #13 testing");

  for (const surface of ["terminal", "desktop"] as const) {
    const band = await $.ui.mount({
      plugin: "sluice-board",
      surface,
      component: "AbovePrompt",
      props: BAND,
    });
    expect(
      await band.find({
        type: "Text",
        text: "sluice main · 3 submissions · testing main+#12+#13 2m10s/15m · #14 waiting · last #9 blamed on 4f1c2aa",
      }),
    ).toBeDefined();
    await band.unmount();

    const pane = await $.ui.mount({
      plugin: "sluice-board",
      surface,
      component: "Pane",
      props: PANE,
      requestId: "sluice",
    });
    expect(
      await pane.find({ type: "Text", text: /#13 testing\s+plugin-board claude\s+← you/ }),
    ).toBeDefined();
    expect(
      await pane.find({
        type: "Text",
        text: /3 submissions · 1 combination under test \(up to 4 at once\)/,
      }),
    ).toBeDefined();
    expect(
      await pane.find({ type: "Text", text: /▶ testing main\+#12\+#13\s+2m10s \/ 15m/ }),
    ).toBeDefined();
    // A combination whose runner lists no tasks is a plain line, nothing to press.
    expect(await pane.find({ type: "Button", key: "tasks-main-c41" })).toBeUndefined();
    // Headed as the herdr plugin's popup is.
    const heads = await pane.findAll({ type: "Text", text: /^(TESTING|QUEUED|RECENT)$/ });
    expect(heads.map((h) => h.text)).toEqual(["TESTING", "QUEUED", "RECENT"]);
    const link = await pane.find({ type: "Link" });
    expect(link?.props.href).toBe("https://github.com/agentplot/sluice/pull/41");
    await pane.unmount();
  }
});

test("a branch that lands on another remote links its pull requests there", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const d = overview();
  d.branches[0]!.remote = "upstream";
  // Two documents name it: it is resolved once.
  host(
    on,
    [d, d],
    nothingSeen(),
    {},
    {
      "git remote get-url origin": "git@github.com:fork-owner/sluice.git\n",
      "git remote get-url upstream": "https://github.com/agentplot/sluice.git\n",
    },
  );
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(ran.filter((r) => r === "git remote get-url upstream")).toHaveLength(1);
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const link = await pane.find({ type: "Link", text: "PR 41" });
  expect(link?.props.href).toBe("https://github.com/agentplot/sluice/pull/41");
  await pane.unmount();
});

/** The pane of a host whose overview is `d`, its remotes answering as `more` says. */
async function paneOf($: Engine, on: On, d: Overview, more: Record<string, string> = {}) {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [d], nothingSeen(), {}, more);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  return $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
}

const SLUICE = "https://github.com/agentplot/sluice";

/** A link's own words: the kit's `text` of a Link leads with its address. */
function words(l: { children: unknown[] }): string {
  return l.children.map(String).join("");
}

test("a landing on GitHub links its commit, and each pull request it landed", async ($, on) => {
  const pane = await paneOf($, on, withLinks("origin"));
  const links = (await pane.findAll({ type: "Link" })).map((l) => [words(l), l.props.href]);
  expect(links).toEqual([
    // QUEUED: #14.
    ["PR 41", `${SLUICE}/pull/41`],
    // RECENT, newest first: #9's blame, then #11 and #12's landing, #12 from no pull request.
    ["PR 40", `${SLUICE}/pull/40`],
    ["9ecc34a", `${SLUICE}/commit/9ecc34a1b2`],
    ["PR 41", `${SLUICE}/pull/41`],
  ]);
  // Both in the landing's own row.
  const row = JSON.stringify(
    (await pane.find({ type: "Text", text: /✓ 19:41 #11 #12 landed as / }))?.children,
  );
  expect(row).toContain(`${SLUICE}/commit/9ecc34a1b2`);
  expect(row).toContain(`${SLUICE}/pull/41`);
  await pane.unmount();
});

test("a landing with no remote, or one off GitHub, names its commit as plain text", async ($, on) => {
  const pane = await paneOf($, on, withLinks());
  expect(
    await pane.find({ type: "Text", text: /✓ 19:41 #11 #12 landed as 9ecc34a/ }),
  ).toBeDefined();
  expect(await pane.find({ type: "Link", text: "9ecc34a" })).toBeUndefined();
  await pane.unmount();
});

test("a branch whose remote is off GitHub links nothing", async ($, on) => {
  const pane = await paneOf($, on, withLinks("mirror"), {
    "git remote get-url mirror": "https://git.example.test/sluice.git\n",
  });
  expect(
    await pane.find({ type: "Text", text: /✓ 19:41 #11 #12 landed as 9ecc34a$/ }),
  ).toBeDefined();
  expect(await pane.findAll({ type: "Link" })).toEqual([]);
  await pane.unmount();
});

test("a blamed pull request is linked beside its row, which still opens its tests", async ($, on) => {
  const pane = await paneOf($, on, withLinks("origin"));
  const pr = await pane.find({ type: "Link", text: "PR 40" });
  expect(pr?.props.href).toBe(`${SLUICE}/pull/40`);
  // Beside the button, never inside it.
  const button = await pane.find({ type: "Button", key: "blamed-main-9-2026-10-16T19:42:00Z" });
  expect(JSON.stringify(button?.children)).not.toContain("PR");
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: "last line" })).toBeDefined();
  await pane.unmount();
});

test("a blame whose number another branch holds opens its own branch's tests", async ($, on) => {
  // #9 blamed on main, and another #9 queued on feature/a (record 0114).
  const d = overview();
  const other = JSON.parse(JSON.stringify(d.branches[0]!)) as Overview["branches"][number];
  other.branch = "feature/a";
  other.candidates = [];
  other.recent = [];
  other.depth = 1;
  other.submissions = [{ id: 9, state: "queued", from: { agent: "claude", worktree: "unit/a" } }];
  d.branches.push(other);
  const pane = await paneOf($, on, d, {
    "sluice status 9 --branch feature/a --json": JSON.stringify({
      id: 9,
      failed: [{ task: "other:test", log: OTHER }],
    }),
  });
  ran.length = 0;
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  expect(ran).toEqual(["sluice status 9 --branch main --json", `tail -n 20 -- ${LOG}`]);
  expect(await pane.find({ type: "Text", text: "last line" })).toBeDefined();
  await pane.unmount();
});

test("a sluice older than where submissions came from names no pull request on RECENT", async ($, on) => {
  const pane = await paneOf($, on, withLinks("origin", false));
  const links = (await pane.findAll({ type: "Link" })).map(words);
  expect(links).toEqual(["PR 41", "9ecc34a"]);
  await pane.unmount();
});

test("PREVIEWS: each preview's name, branch@rev7 and state, and its addresses linked", async ($, on) => {
  const pane = await paneOf($, on, withPreviews());
  const heads = await pane.findAll({ type: "Text", text: /^(CHECKING|PREVIEWS|RECENT)$/ });
  expect(heads.map((h) => h.text)).toEqual(["RECENT", "PREVIEWS"]);
  const up = await pane.find({
    type: "Text",
    text: /^ {2}preview\/4f1c2aa0e1b2 {2}main@4f1c2aa {2}up {2}web: /,
  });
  expect(up).toBeDefined();
  const link = await pane.find({ type: "Link", text: "web-4f1c2aa.example.test" });
  expect(link?.props.href).toBe("https://web-4f1c2aa.example.test");
  expect(words(link!)).toBe("https://web-4f1c2aa.example.test");
  expect(JSON.stringify(up?.children)).toContain("https://web-4f1c2aa.example.test");
  // One whose policy names no addresses: its row, and no link.
  expect(
    await pane.find({ type: "Text", text: "  preview/9ecc34a1b2c3  main@9ecc34a  starting" }),
  ).toBeDefined();
  expect(
    (await pane.findAll({ type: "Link" })).filter((l) =>
      /example\.test/.test(String(l.props.href)),
    ),
  ).toHaveLength(1);
  expect(await pane.find({ type: "Text", text: /0 worktrees held · 2 previews/ })).toBeDefined();
  await pane.unmount();
});

test("a link that is not https: is drawn as a link on every surface, the engine plain on desktop", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [withPreviews("http://localhost:4001")], nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // The kit reports the tree a surface is handed, never what it paints: on
  // desktop the engine draws this link's text plain (LinkProps.href), and the
  // tree keeps the Link. So the test pins what the kit marks, the same Link
  // handed to both surfaces and taken by each; an engine that refused it, or
  // a kit that marked it plain, fails here rather than opening nothing.
  for (const surface of ["desktop", "terminal"] as const) {
    const pane = await $.ui.mount({
      plugin: "sluice-board",
      surface,
      component: "Pane",
      props: PANE,
      requestId: "sluice",
    });
    await expect(pane.drawn()).resolves.toBeDefined();
    const row = await pane.find({ type: "Text", text: /web: / });
    expect(row?.children.slice(-2)).toEqual([
      "  web: ",
      {
        type: "Link",
        props: { href: "http://localhost:4001" },
        children: ["http://localhost:4001"],
      },
    ]);
    await pane.unmount();
  }
});

test("no PREVIEWS while there are none", async ($, on) => {
  const pane = await paneOf($, on, overview());
  expect(await pane.find({ type: "Text", text: "PREVIEWS" })).toBeUndefined();
  await pane.unmount();
});

test("the pane leaves out branches with nothing queued and no event this quarter hour", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const d = overview();
  const idle = (branch: string, recent: Overview["branches"][number]["recent"]) => ({
    ...d.branches[0]!,
    branch,
    depth: 0,
    submissions: [],
    candidates: [],
    recent,
  });
  for (let i = 0; i < 30; i++) d.branches.push(idle(`feature/b${i}`, []));
  const landed = (at: string) => [
    { schema: "sluice/log@1", at, event: "landed" as const, ids: [77], to: "abcdef0" },
  ];
  d.branches.push(idle("feature/done", landed("2026-10-16T19:40:00Z")));
  d.branches.push(idle("feature/old", landed("2026-10-16T19:20:00Z")));
  host(on, [d], { prompts: [], toasts: [], status: [] });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  expect(await pane.find({ type: "Text", text: /^main at / })).toBeDefined();
  expect(await pane.find({ type: "Text", text: /^feature\/done at / })).toBeDefined();
  expect(await pane.findAll({ type: "Text", text: /feature\/(b\d|old)/ })).toEqual([]);
  expect(
    await pane.find({ type: "Text", text: "31 other branches with nothing queued" }),
  ).toBeDefined();
  await pane.unmount();
});

test("a combination's tasks: the count and bar in the band, the list in the pane on a press", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  host(on, [withTasks()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();

  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  expect(
    await band.find({ type: "Text", text: /testing main\+#12\+#13 3\/5 ▓▓▓▓▓▓░░░░ 2m10s\/15m/ }),
  ).toBeDefined();
  expect(await band.find({ type: "Text", text: /sluice:e2e/ })).toBeUndefined();
  await band.unmount();

  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  expect(
    await pane.find({
      type: "Text",
      text: /▶ testing main\+#12\+#13\s+3\/5 ▓▓▓▓▓▓░░░░\s+2m10s \/ 15m/,
    }),
  ).toBeDefined();
  // Folded until its row is pressed.
  const tasks = /^\s+[✓✗…·] (sluice|core|site):/;
  expect(await pane.findAll({ type: "Text", text: tasks })).toEqual([]);
  expect(await pane.find({ type: "Button", key: "tasks-main-c41" })).toBeDefined();
  await pane.press({ key: "tasks-main-c41" });
  await pane.redraw();
  const lines = (await pane.findAll({ type: "Text", text: tasks })).map((t) => t.text);
  const at = (re: RegExp) => lines.findIndex((l) => re.test(l));
  expect(at(/✓ sluice:test\s+1m48s$/)).toBeGreaterThanOrEqual(0);
  expect(at(/… sluice:e2e\s+running 1m12s$/)).toBeGreaterThan(at(/✓ sluice:test/));
  expect(at(/· core:test\s+waiting$/)).toBeGreaterThan(at(/… sluice:e2e/));
  await pane.press({ key: "tasks-main-c41" });
  await pane.redraw();
  expect(await pane.findAll({ type: "Text", text: tasks })).toEqual([]);
  await pane.unmount();

  // The band is as it was, whatever the pane opened.
  const after = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  expect(
    await after.find({ type: "Text", text: /testing main\+#12\+#13 3\/5 ▓▓▓▓▓▓░░░░ 2m10s\/15m/ }),
  ).toBeDefined();
  expect(await after.find({ type: "Text", text: /sluice:e2e/ })).toBeUndefined();
  await after.unmount();
});

test("agents' checks: the clause in the band, CHECKING and the host line in the pane", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  host(on, [withChecks()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.status).toContain(
    "sluice: 3 submissions · yours #13 testing · 1 slot to an agent's checks",
  );

  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  expect(
    await band.find({ type: "Text", text: /#14 waiting · .* · 1 slot to an agent's checks$/ }),
  ).toBeDefined();
  expect(await band.find({ type: "Text", text: /fix-cards/ })).toBeUndefined();
  await band.unmount();

  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({ type: "Text", text: /^(TESTING|QUEUED|RECENT|CHECKING)$/ });
  expect(heads.map((h) => h.text)).toEqual(["TESTING", "QUEUED", "RECENT", "CHECKING"]);
  const running = await pane.find({ type: "Text", text: /fix-cards → main 3\/8 ▓▓▓░░░░░░░ 1m10s/ });
  // Plain, never the yellow of a combination under test.
  expect(running?.props.color).toBeUndefined();
  expect(await pane.find({ type: "Text", text: /e2e-stack → main make e2e 2m00s/ })).toBeDefined();
  expect(
    await pane.find({ type: "Text", text: /done-ok → main 14 passed · reused when submitted/ }),
  ).toBeDefined();
  expect(
    await pane.find({
      type: "Text",
      text: /dirty → main 14 passed · not kept: uncommitted changes/,
    }),
  ).toBeDefined();
  expect(await pane.find({ type: "Text", text: /broke → main 2 of 14 failed/ })).toBeDefined();
  expect(await pane.find({ type: "Text", text: /long-ago/ })).toBeUndefined();
  expect(
    await pane.find({ type: "Text", text: "host: 2 of 2 runs · 1 queue · 1 agent" }),
  ).toBeDefined();
  // A check is never a submission: QUEUED lists #14 alone.
  expect(await pane.findAll({ type: "Text", text: /^\s+#\d+ queued/ })).toHaveLength(1);
  // The worktree only warned of holds no ports, and is not counted held.
  expect(await pane.find({ type: "Text", text: "6 worktrees held · 0 previews" })).toBeDefined();
  await pane.unmount();
});

test("a CHECKING row opens to its run's tasks, and folds", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [withChecks()], { prompts: [], toasts: [], status: [] });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const tasks = async () =>
    (await pane.findAll({ type: "Text", text: /^\s+[✓✗…·] t:\d+\s/ })).map((t) => t.text.trim());
  const press = async (name: string) => {
    await pane.press({ key: `checks-/work/sluice/${name}` });
    await pane.redraw();
  };
  expect(await tasks()).toEqual([]);

  // A check under way: its eight tasks, marked and ordered as a combination's.
  await press("fix-cards");
  const going = await tasks();
  expect(going).toHaveLength(8);
  expect(going.slice(0, 4).map((t) => t.split(" ")[0])).toEqual(["✓", "✓", "✓", "…"]);
  expect(going.slice(4).every((t) => t.startsWith("· "))).toBe(true);
  await press("fix-cards");
  expect(await tasks()).toEqual([]);

  // A command check has none.
  await press("e2e-stack");
  expect(await pane.find({ type: "Text", text: "runs make e2e · no tasks" })).toBeDefined();
  await press("e2e-stack");
  expect(await pane.find({ type: "Text", text: /no tasks/ })).toBeUndefined();

  // A check that ended: the run it ended with.
  await press("done-ok");
  expect(await tasks()).toHaveLength(14);
  await press("done-ok");
  expect(await tasks()).toEqual([]);
  await pane.unmount();
});

test("the pane keeps the overview's order of branches, and counts the idle ones", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const d = overview();
  const main = d.branches[0]!;
  const idle = {
    ...main,
    branch: "feature/a",
    depth: 0,
    submissions: [],
    candidates: [],
    recent: [],
  };
  d.branches = [{ ...main, branch: "release" }, idle, main];
  host(on, [d], { prompts: [], toasts: [], status: [] });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({ type: "Text", text: /^\S+ at / });
  expect(heads.map((h) => h.text.split(" ")[0])).toEqual(["release", "main"]);
  expect(
    await pane.find({ type: "Text", text: "1 other branch with nothing queued" }),
  ).toBeDefined();
  await pane.unmount();
});

test("a branch named by SLUICE_BRANCH on the status line, and first in the band and the pane", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  const d = withRelease();
  d.branches[0]!.depth = 1;
  d.branches[0]!.submissions = [
    { id: 20, state: "queued", from: { agent: "claude", worktree: "w", path: "/work/sluice/w" } },
  ];
  host(on, [d], seen, { SLUICE_BRANCH: "release" });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.status).toContain("sluice: release · 3 queued · #12 testing 3/8 · 2 checking");
  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  const lines = await band.findAll({ type: "Text", text: /^sluice / });
  expect(lines.map((l) => l.text.split(" · ")[0])).toEqual(["sluice release", "sluice main"]);
  await band.unmount();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({ type: "Text", text: /^\S+ at / });
  expect(heads.map((h) => h.text.split(" ")[0])).toEqual(["release", "main"]);
  await pane.unmount();
});

test("the pane shows a named branch first even when idle", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const d = overview();
  const main = d.branches[0]!;
  const idle = {
    ...main,
    branch: "feature/a",
    depth: 0,
    submissions: [],
    candidates: [],
    recent: [],
  };
  d.branches = [{ ...main, branch: "release" }, idle, main];
  host(on, [d], { prompts: [], toasts: [], status: [] }, { SLUICE_BRANCH: "feature/a" });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({ type: "Text", text: /^\S+ at / });
  expect(heads.map((h) => h.text.split(" ")[0])).toEqual(["feature/a", "release", "main"]);
  expect(await pane.find({ type: "Text", text: /with nothing queued/ })).toBeUndefined();
  await pane.unmount();
});

test("RELEASE FAILED: each held place first under its branch, its command beneath, nothing to press", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [withHeld()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.status).toContain(
    "sluice: 3 submissions · yours #13 testing · 2 held: release failed",
  );
  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  expect(
    await band.find({ type: "Text", text: /#14 waiting · .* · 2 held: release failed$/ }),
  ).toBeDefined();
  await band.unmount();

  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({
    type: "Text",
    text: /^(RELEASE FAILED|TESTING|QUEUED|RECENT)$/,
  });
  expect(heads.map((h) => h.text)).toEqual(["RELEASE FAILED", "TESTING", "QUEUED", "RECENT"]);
  // The herdr popup's words, line for line.
  const rows = [
    [
      "✗ sluice/try/main/48 held · failed 4 times · usage: scripts/preview up",
      "sluice release sluice/try/main/48 · --accept-failure frees it without",
    ],
    [
      "✗ worktree unit-a held · failed once · rm: cannot remove .stack",
      "sluice release --worktree /work/sluice/unit-a · --accept-failure frees it without",
    ],
  ];
  for (const [what, how] of rows) {
    expect((await pane.find({ type: "Text", text: `  ${what}` }))?.props.color).toBe("red");
    expect((await pane.find({ type: "Text", text: `    ${how}` }))?.props.dimColor).toBe(true);
  }
  // One still being tried is not shown.
  expect(await pane.find({ type: "Text", text: /sluice\/try\/main\/51/ })).toBeUndefined();
  // Rows that open are buttons; a held place's are not.
  expect(await pane.find({ type: "Button", text: /blamed on 4f1c2aa/ })).toBeDefined();
  expect(await pane.find({ type: "Button", text: /held|--accept-failure/ })).toBeUndefined();
  await pane.unmount();
});

test("an idle branch with a held place is shown, and one with no records after the rest", async ($, on) => {
  const d = withHeld();
  d.branches[0] = { ...d.branches[0]!, depth: 0, candidates: [], submissions: [], recent: [] };
  d.sessions[0]!.branch = "release";
  const pane = await paneOf($, on, d);
  const heads = await pane.findAll({ type: "Text", text: /^\S+ (at|·) / });
  expect(heads.map((h) => h.text)).toEqual([
    expect.stringMatching(/^main at /),
    "release · no records",
  ]);
  expect(await pane.find({ type: "Text", text: "No branch has a queue." })).toBeUndefined();
  expect(await pane.findAll({ type: "Text", text: "RELEASE FAILED" })).toHaveLength(2);
  expect(
    await pane.find({
      type: "Text",
      text: "  ✗ worktree unit-a held · failed once · rm: cannot remove .stack",
    }),
  ).toBeDefined();
  await pane.unmount();
});

test("an overview from before held places draws no RELEASE FAILED", async ($, on) => {
  const pane = await paneOf($, on, overview());
  expect(await pane.find({ type: "Text", text: "RELEASE FAILED" })).toBeUndefined();
  expect(await pane.find({ type: "Text", text: /held · failed/ })).toBeUndefined();
  await pane.unmount();
});

test("the pane names a branch it has no records of", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [overview()], { prompts: [], toasts: [], status: [] }, { SLUICE_BRANCH: "hotfix" });
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const heads = await pane.findAll({ type: "Text", text: /^\S+ (at|·) / });
  expect(heads.map((h) => h.text)).toEqual([
    "hotfix · no records",
    expect.stringMatching(/^main at /),
  ]);
  await pane.unmount();
});

test("a blame of this session's submission is a toast, and one turn in sluice's words", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [overview(), blamed()], seen);
  // The first overview's collect finds nothing pending; the blame's finds it.
  inbox.answers = [printing(""), printing(BLAME13)];
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.toasts).toEqual(["sluice: #13 was blamed"]);
  expect(seen.prompts).toEqual([BLAME13]);
  // Run in the session's worktree: everything owed at the first overview,
  // then only what nothing has handed over; the board reads no record of
  // #13 for the turn.
  expect(inbox.ranIn).toEqual([ROOT, ROOT]);
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW]);
  expect(ran).not.toContain("sluice status 13 --branch main --json");
});

test("under a multiplexer the notice has come: a toast, and no turn", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [queued13(), landed13()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(inbox.ranIn).toHaveLength(2);
  expect(seen.toasts).toEqual(["sluice: #13 landed on main"]);
  expect(seen.prompts).toEqual([]);
});

test("another session's landing is no toast and no collect", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const after = overview();
  after.branches[0]!.recent.push({
    schema: "sluice/log@1",
    at: "2026-10-16T19:51:00Z",
    event: "landed",
    ids: [20],
    to: "aaaaaaa",
  });
  host(on, [overview(), after], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // The first overview's collect alone.
  expect(inbox.ranIn).toHaveLength(1);
  expect(seen.toasts).toEqual([]);
  expect(seen.prompts).toEqual([]);
});

test("the session's start collects once, and its turn is sluice's words, with no toast", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [landed13()], seen);
  inbox.answers = [printing(LANDED13)];
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(inbox.ranIn).toEqual([ROOT]);
  expect(seen.prompts).toEqual([LANDED13]);
  expect(seen.toasts).toEqual([]);
});

test("nothing pending at the session's start starts no turn", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [landed13()], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(inbox.ranIn).toHaveLength(1);
  expect(seen.prompts).toEqual([]);
});

test(
  "the wake can be turned off: a toast, and nothing collected",
  { options: { wake: false } },
  async ($, on) => {
    const clock = mock.clock(on);
    await clock.set(NOW);
    const seen = nothingSeen();
    host(on, [overview(), blamed()], seen);
    inbox.answers = [printing(BLAME13)];
    await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
    await clock.settle();
    expect(inbox.ranIn).toEqual([]);
    expect(seen.prompts).toEqual([]);
    expect(seen.toasts).toContain("sluice: #13 was blamed");
  },
);

test("a branch sluice could not read: the turn, and a toast of sluice's line", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [queued13(), landed13()], seen);
  const unread = "sluice: could not read feature/x's records: it is gone";
  inbox.answers = [printing(""), { exitCode: 3, stdout: `${LANDED13}\n`, stderr: `${unread}\n` }];
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.prompts).toEqual([LANDED13]);
  expect(seen.toasts).toEqual(["sluice: #13 landed on main", unread]);
});

test("a sluice with no inbox: no turn, and a toast in its words", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  host(on, [queued13()], seen);
  const refused = "error: unrecognized subcommand 'inbox'\n\nUsage: sluice <COMMAND>\n";
  inbox.answers = [{ exitCode: 2, stdout: "", stderr: refused }];
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(seen.prompts).toEqual([]);
  expect(seen.toasts).toEqual([
    "sluice: couldn't collect this session's outcomes: error: unrecognized subcommand 'inbox'",
  ]);
});

const GONE = {
  error: { kind: "internal", message: "reading the records of main: it is gone", next: [] },
};

test("an overview's error is one line in the band, from --watch, until a document comes", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const reads: string[] = [];
  on("session.start", ($, e) => ({ cwd: e.cwd }));
  on("command.register", ($, e) => ({ value: { command: e.name } }));
  on("process.run", ($, e) => {
    const argv = e.argv.join(" ");
    if (argv === READ) reads.push(argv);
    if (argv === "git rev-parse --show-toplevel")
      return { value: { exitCode: 0, stdout: `${ROOT}\n`, stderr: "", ...WHOLE } };
    return { value: { exitCode: 1, stdout: "", stderr: `no ${argv}`, ...WHOLE } };
  });
  const watch = watches(on, clock);
  on("ui.status", () => ({ value: undefined }));
  const band = async () => {
    const b = await $.ui.mount({
      plugin: "sluice-board",
      surface: "terminal",
      component: "AbovePrompt",
      props: BAND,
    });
    const texts = (await b.findAll({ type: "Text" })).map((t) => t.text);
    await b.unmount();
    return texts;
  };
  const QUEUE = /^sluice main · 3 submissions/;

  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // The watch prints a document, then fails.
  await watch.print(overview());
  await watch.print(GONE);
  await watch.end();
  expect(await band()).toEqual(["sluice: reading the records of main: it is gone"]);

  // The watch started again prints a document, then fails.
  await clock.advance(1000);
  await watch.print(overview());
  expect((await band()).some((t) => QUEUE.test(t))).toBe(true);
  await watch.print(GONE);
  await watch.end();
  expect(await band()).toEqual(["sluice: reading the records of main: it is gone"]);

  // A watch that prints a line it can't read leaves the band as it was, and stays open.
  await clock.advance(2000);
  await watch.say("not a document\n");
  expect(await band()).toEqual(["sluice: reading the records of main: it is gone"]);
  expect(watch.started).toHaveLength(3);
  expect(watch.isOpen()).toBe(true);
  expect(reads).toEqual([]);
});

/** The engine's record of the open panes, and of each open the board asked for. */
function panes(on: On) {
  const open = new Set<string>();
  const opens: { closeOnEscape?: true; focus?: true }[] = [];
  const closes: string[] = [];
  on("ui.open", ($, e) => {
    opens.push(e);
    open.add(e.id);
    return { value: { isPlaced: true as const } };
  });
  on("ui.close", ($, e) => {
    closes.push(e.id);
    open.delete(e.id);
    return { value: undefined };
  });
  on("ui.panes", () => ({
    value: [...open].map((id) => ({
      id,
      title: id,
      isShown: true,
      isFocused: false,
      isPlaced: true,
    })),
  }));
  return { open, opens, closes };
}

/** `/sluice` typed in the composer, full-screen or not. */
function slash(isFullscreen: boolean) {
  return {
    command: "sluice",
    args: "",
    origin: { kind: "composer" as const },
    presentation: { isFullscreen, columns: isFullscreen ? 200 : 100 },
  };
}

test("/sluice opens the pane to close on Escape, and closes it when open", async ($, on) => {
  host(on, [overview()], nothingSeen());
  const { open, opens } = panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });

  expect((await $.command.run(slash(true))).text).toBe(
    "sluice's queue is open in a pane. Its close mark or /sluice again closes it.",
  );
  expect([...open]).toEqual(["sluice"]);
  expect(opens.map((o) => o.closeOnEscape)).toEqual([true]);
  // Docked, the prompt keeps the keyboard.
  expect(opens.map((o) => o.focus)).toEqual([undefined]);
  expect((await $.command.run(slash(true))).text).toBe("sluice's pane is closed.");
  expect([...open]).toEqual([]);
  await $.command.run(slash(true));
  expect([...open]).toEqual(["sluice"]);
  expect(opens.length).toBe(2);
});

test("/sluice above the prompt asks for the keyboard and says how to close the pane", async ($, on) => {
  host(on, [overview()], nothingSeen());
  const { open, opens } = panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });

  expect((await $.command.run(slash(false))).text).toBe(
    "sluice's queue is open above the prompt. Esc, its close button or /sluice again closes it.",
  );
  expect([...open]).toEqual(["sluice"]);
  expect(opens.map((o) => [o.focus, o.closeOnEscape])).toEqual([[true, true]]);
  expect((await $.command.run(slash(false))).text).toBe("sluice's pane is closed.");
  expect([...open]).toEqual([]);
});

test("a pane above the prompt has a close button, a docked one none of the board's", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [overview()], nothingSeen());
  const { closes } = panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();

  const inline = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: INLINE,
    requestId: "sluice",
  });
  const button = await inline.find({ type: "Button", key: "close" });
  expect(button?.props).toMatchObject({ role: "dismiss", hotkey: "x", label: "close" });
  await inline.press({ key: "close" });
  expect(closes).toEqual(["sluice"]);
  await inline.unmount();

  const docked = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  expect(await docked.find({ key: "close" })).toBeUndefined();
  await docked.unmount();
});

/** main with #3 queued and nothing else: a repository just set up, with one submission. */
function justSetUp(): Overview {
  const d = overview();
  d.branches = [
    {
      ...d.branches[0]!,
      depth: 1,
      candidates: [],
      recent: [],
      submissions: [{ id: 3, state: "queued", from: { agent: "claude", worktree: "w3" } }],
    },
  ];
  return d;
}

/** The pane as `/sluice` drew it, docked unless `props` says otherwise. */
function mountPane($: Engine, props = PANE) {
  return $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props,
    requestId: "sluice",
  });
}

/** The band's lines, as it draws them now. */
async function bandOf($: Engine): Promise<string[]> {
  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  const lines = (await band.findAll({ type: "Text" })).map((t) => t.text);
  await band.unmount();
  return lines;
}

const ASK = "Ask Claude to set sluice up in this repository.";

/** The texts of the pane as `/sluice` drew it, docked. */
async function paneTexts($: Engine): Promise<string[]> {
  const pane = await mountPane($);
  const texts = (await pane.findAll({ type: "Text" })).map((t) => t.text);
  await pane.unmount();
  return texts;
}

test("sluice not installed: the pane says the command was not found, and what to do", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  watch.failing = true;
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await $.command.run(slash(false));
  const pane = await mountPane($, INLINE);
  expect((await pane.findAll({ type: "Text" })).map((t) => t.text)).toEqual([
    "sluice could not be read here: the sluice command was not found",
    ASK,
  ]);
  expect(await pane.find({ type: "Button", key: "close" })).toBeDefined();
  await pane.unmount();
  expect(await bandOf($)).toEqual([]);
});

test("the overview fails: the pane gives the watch's own words, and what to do", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // Reading, until the watch prints or ends.
  expect(await paneTexts($)).toEqual(["Reading sluice…"]);
  await watch.exit(3, "sluice: not a git repository\n");
  await $.command.run(slash(true));
  expect(await paneTexts($)).toEqual([
    "sluice could not be read here: sluice: not a git repository",
    ASK,
  ]);
  expect(await bandOf($)).toEqual([]);
  // One that writes nothing gives its exit.
  await clock.advance(1000);
  await watch.exit(4, "");
  expect((await paneTexts($))[0]).toBe("sluice could not be read here: sluice overview exited 4");
});

test("the watch prints only its failure: the pane gives its message, until an overview comes", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.say('{"error":{"kind":"internal","message":"no policy on main","next":[]}}\n');
  expect(watch.isOpen()).toBe(true);
  expect(await paneTexts($)).toEqual(["sluice could not be read here: no policy on main", ASK]);
  expect(await bandOf($)).toEqual(["sluice: no policy on main"]);
  // Its message stands over what it wrote to stderr when it ends.
  await watch.exit(1, "sluice: interrupted\n");
  expect((await paneTexts($))[0]).toBe("sluice could not be read here: no policy on main");
  // The next watch prints the queue: the reason goes.
  await clock.advance(1000);
  await watch.print(justSetUp());
  expect(await paneTexts($)).toContain("QUEUED");
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  // A failure after it, and the watch's end, leave the queue in the pane.
  await watch.say('{"error":{"kind":"internal","message":"no policy on main","next":[]}}\n');
  await watch.exit(3, "sluice: not a git repository\n");
  expect(await paneTexts($)).toContain("QUEUED");
});

test("sluice set up during the session: /sluice starts the watch at once, which shows the queue", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  watch.failing = true;
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  watch.failing = false;
  await $.command.run(slash(true));
  await clock.settle();
  // Started at once, the clock unmoved, and nothing read in its place.
  expect(watch.started).toEqual([NOW, NOW]);
  expect(watch.isOpen()).toBe(true);
  await watch.print(justSetUp());
  const pane = await mountPane($);
  const heads = await pane.findAll({ type: "Text", text: /^(TESTING|QUEUED|RECENT)$/ });
  expect(heads.map((h) => h.text)).toEqual(["QUEUED"]);
  expect(await pane.find({ type: "Text", text: /#3 queued\s+w3 claude/ })).toBeDefined();
  await pane.unmount();
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  expect(ran.filter((r) => r.startsWith("sluice overview"))).toEqual([]);
  // With the queue shown, /sluice waits the watch's wait out like any other.
  await watch.end();
  await $.command.run(slash(true));
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toHaveLength(2);
  await startedAgainAfter(clock, watch, 2000);
});

test("still failing: /sluice tries the watch again at once, and the pane says why", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  watch.failing = true;
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toEqual([NOW, NOW]);
  expect(await paneTexts($)).toEqual([
    "sluice could not be read here: the sluice command was not found",
    ASK,
  ]);
  // Cutting a wait short is the user's asking: the next wait grows all the same.
  await startedAgainAfter(clock, watch, 2000);
  // Two at once start one.
  await $.command.run(slash(true));
  await Promise.all([$.command.run(slash(true)), $.command.run(slash(true))]);
  await clock.settle();
  expect(watch.started).toHaveLength(4);
});

test("a watch already open: /sluice starts no second one", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toEqual([NOW]);
  expect(watch.isOpen()).toBe(true);
  expect(await paneTexts($)).toEqual(["Reading sluice…"]);
});

/** What a sluice older than its watch writes, refusing the command or the argument. */
const NO_OVERVIEW =
  "error: unrecognized subcommand 'overview'\n\nUsage: sluice <COMMAND>\n\nFor more information, try '--help'.\n";
const NO_WATCH =
  "error: unexpected argument '--watch' found\n\nUsage: sluice overview [OPTIONS]\n\nFor more information, try '--help'.\n";
const VERSION = "sluice --version";

test("a watch refused for having no watch asks sluice its version, and no other end does", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen(), { [VERSION]: "sluice 0.11.2\n" });
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // Killed, exited with sluice's words, could not be started: no version asked.
  await watch.end();
  await clock.advance(1000);
  await watch.exit(3, "sluice: not a git repository\n");
  watch.failing = true;
  await clock.advance(2000);
  watch.failing = false;
  await clock.advance(4000);
  await watch.exit(2, "error: unexpected argument '--json' found\n");
  expect(watch.started).toHaveLength(4);
  expect(ran.filter((r) => r === VERSION)).toEqual([]);
  // Refused as a sluice with no `overview`, then with no `--watch`: once each.
  await clock.advance(8000);
  await watch.exit(2, NO_OVERVIEW);
  expect(ran.filter((r) => r === VERSION)).toEqual([VERSION]);
  watch.refusing = NO_WATCH;
  await clock.advance(16000);
  expect(watch.started).toHaveLength(6);
  expect(ran.filter((r) => r === VERSION)).toEqual([VERSION, VERSION]);
  // /sluice, where the board can't follow, starts the next watch at once.
  watch.refusing = null;
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toHaveLength(7);
  expect(watch.isOpen()).toBe(true);
  expect(ran.filter((r) => r.startsWith("sluice overview"))).toEqual([]);
});

const UPGRADE = "Ask Claude to upgrade sluice.";
const OLD = "can't follow sluice 0.11.2: it has no watch; the board needs sluice 0.12 or later";

test("an older sluice on the host: the band and the pane say the board can't follow it", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen, { [VERSION]: "sluice 0.11.2\n" });
  watch.refusing = NO_OVERVIEW;
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(await bandOf($)).toEqual([OLD]);
  expect(seen.status).toEqual([undefined]);
  await $.command.run(slash(false));
  const pane = await mountPane($, INLINE);
  expect((await pane.findAll({ type: "Text" })).map((t) => t.text)).toEqual([OLD, UPGRADE]);
  expect(await pane.find({ type: "Button", key: "close" })).toBeDefined();
  await pane.unmount();
  // The board keeps trying the watch, and reads nothing in its place.
  await startedAgainAfter(clock, watch, 2000);
  expect(await bandOf($)).toEqual([OLD]);
  expect(ran.filter((r) => r.startsWith("sluice overview"))).toEqual([]);
});

test("no version to name: the band says this sluice", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.exit(2, NO_WATCH);
  expect(ran).toContain(VERSION);
  const line = "can't follow this sluice: it has no watch; the board needs sluice 0.12 or later";
  expect(await bandOf($)).toEqual([line]);
  expect(await paneTexts($)).toEqual([line, UPGRADE]);
});

test("the host's sluice replaced by one with no watch: the line takes the queue's place", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen, { [VERSION]: "sluice 0.11.2\n" });
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(justSetUp());
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  expect(seen.status.at(-1)).toBe("sluice: 1 submission");
  await watch.end();
  watch.refusing = NO_OVERVIEW;
  await clock.advance(1000);
  expect(await bandOf($)).toEqual([OLD]);
  expect(await paneTexts($)).toEqual([OLD, UPGRADE]);
  expect(seen.status.at(-1)).toBeUndefined();
});

test("the host's sluice upgraded: the queue comes back, and what was decided meanwhile is collected", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen, { [VERSION]: "sluice 0.11.2\n" });
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // #13, this session's, is queued; then the host's sluice can't be followed.
  inbox.answers = [printing(""), printing(LANDED13)];
  await watch.print(queued13());
  await watch.end();
  watch.refusing = NO_OVERVIEW;
  await clock.advance(1000);
  expect(await bandOf($)).toEqual([OLD]);
  expect(seen.prompts).toEqual([]);
  expect(inbox.ranIn).toHaveLength(1);
  // Upgraded: the next watch prints #13 landed while the board couldn't follow.
  watch.refusing = null;
  await clock.advance(2000);
  expect(watch.isOpen()).toBe(true);
  await watch.print(landed13());
  expect(inbox.ranIn).toHaveLength(2);
  // Decided while no watch was open, it is new: only what nothing handed over.
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW]);
  expect(seen.toasts).toContain("sluice: #13 landed on main");
  expect(seen.prompts).toEqual([LANDED13]);
  const band = await bandOf($);
  expect(band).toHaveLength(1);
  expect(band[0]).toMatch(
    /^sluice main · 2 submissions · #12 #14 waiting · last #13 landed as 5d6e7f8/,
  );
  expect(seen.status.at(-1)).toBe("sluice: 2 submissions");
  expect(await paneTexts($)).toContain("QUEUED");
});

test("the first queue comes from the watch, and nothing reads the records in its place", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(watch.started).toEqual([NOW]);
  expect(await bandOf($)).toEqual([]);
  await watch.print(justSetUp());
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  // Nor does the pane, or a minute's wait.
  await $.command.run(slash(true));
  await clock.advance(60_000);
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  expect(watch.started).toEqual([NOW]);
  expect(ran.filter((r) => r.startsWith("sluice overview"))).toEqual([]);
});

test("a pane with no branch says how a submission gets there", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const none = overview();
  none.branches = [];
  host(on, [none], nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await $.command.run(slash(true));
  const pane = await mountPane($);
  expect(await pane.find({ type: "Text", text: "Nothing submitted yet." })).toBeDefined();
  expect(
    await pane.find({
      type: "Text",
      text: "When an agent in a worktree runs sluice submit <branch>, its submission shows here until it lands.",
    }),
  ).toBeDefined();
  expect(await pane.find({ type: "Text", text: "No branch has a queue." })).toBeUndefined();
  await pane.unmount();
});

test("a pane whose branches have nothing to show says no branch has a queue", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const quiet = overview();
  quiet.branches[0] = {
    ...quiet.branches[0]!,
    depth: 0,
    candidates: [],
    submissions: [],
    recent: [{ schema: "sluice/log@1", at: "2026-10-16T19:20:00Z", event: "landed", ids: [11] }],
  };
  host(on, [quiet], nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await mountPane($);
  expect(await pane.find({ type: "Text", text: "No branch has a queue." })).toBeDefined();
  expect(await pane.find({ type: "Text", text: "Nothing submitted yet." })).toBeUndefined();
  await pane.unmount();
});

test("a named branch with no records keeps its line, and no 'Nothing submitted yet'", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const none = overview();
  none.branches = [];
  host(on, [none], nothingSeen(), { SLUICE_BRANCH: "hotfix" });
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await mountPane($);
  expect(await pane.find({ type: "Text", text: "hotfix · no records" })).toBeDefined();
  expect(await pane.find({ type: "Text", text: "No branch has a queue." })).toBeDefined();
  expect(await pane.find({ type: "Text", text: "Nothing submitted yet." })).toBeUndefined();
  await pane.unmount();
});

/**
 * A host whose watch the test drives, each command it runs kept in `ran`; a
 * command `more` names prints what it says there, and `sluice inbox`
 * answers as `inbox` says.
 */
function watchedHost(
  on: On,
  clock: MockClock,
  seen: { prompts: string[]; toasts: string[]; status: (string | undefined)[] },
  more: Record<string, string> = {},
) {
  inboxAfresh();
  on("session.start", ($, e) => ({ cwd: e.cwd }));
  on("command.register", ($, e) => ({ value: { command: e.name } }));
  on("process.run", ($, e) => {
    const argv = e.argv.join(" ");
    ran.push(argv);
    const out = (stdout: string) => ({ value: { exitCode: 0, stdout, stderr: "", ...WHOLE } });
    const answer = more[argv];
    if (answer !== undefined) return out(answer);
    if (argv === INBOX || argv === INBOX_NEW) return inboxRun(e.init?.cwd, argv);
    if (argv === "git rev-parse --show-toplevel") return out(`${ROOT}\n`);
    return { value: { exitCode: 1, stdout: "", stderr: `no ${argv}`, ...WHOLE } };
  });
  on("ui.status", ($, e) => {
    seen.status.push(e.text);
    return { value: undefined };
  });
  on("ui.toast", ($, e) => {
    seen.toasts.push(e.text);
    return { value: undefined };
  });
  on("prompt.submit", ($, e) => {
    seen.prompts.push(e.text);
    return { text: e.text };
  });
  on("ui.notify", () => ({ value: { isSent: true as const, channel: "terminal_bell" } }));
  // Core's own drawing above the prompt, where the band has nothing to show.
  on("ui.render", { component: "AbovePrompt" }, () => ({ type: "engine" as const, ref: 0 }));
  return watches(on, clock);
}

/** Moves the clock `ms` on, and finds the watch started again then and not before. */
async function startedAgainAfter(clock: MockClock, watch: ReturnType<typeof watches>, ms: number) {
  const n = watch.started.length;
  await clock.advance(ms - 1);
  expect(watch.started).toHaveLength(n);
  await clock.advance(1);
  expect(watch.started).toHaveLength(n + 1);
}

/** The fixture with nothing under test, and #13, this session's, queued. */
function queued13(): Overview {
  const d = overview();
  const main = d.branches[0]!;
  main.candidates = [];
  for (const s of main.submissions) s.state = "queued";
  return d;
}

/** The same, then #13 landed as 5d6e7f8a9b. */
function landed13(): Overview {
  const d = queued13();
  const main = d.branches[0]!;
  main.submissions = main.submissions.filter((s) => s.id !== 13);
  main.depth = 2;
  main.recent.push({
    schema: "sluice/log@1",
    at: "2026-10-16T19:46:00Z",
    event: "landed",
    ids: [13],
    to: "5d6e7f8a9b",
  });
  return d;
}

const READ = "sluice overview --json";

test("a watch that ends is started again a second later, and what it prints is collected", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  inbox.answers = [printing(""), printing(LANDED13)];
  await watch.print(queued13());
  await clock.advance(5000);
  await watch.end();

  await startedAgainAfter(clock, watch, 1000);
  expect(watch.started[1]! - watch.ended[0]!).toBe(1000);
  await watch.print(landed13());
  expect(inbox.ranIn).toHaveLength(2);
  // Landed while no watch was open: only what nothing handed over.
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW]);
  expect(seen.toasts).toContain("sluice: #13 landed on main");
  expect(seen.prompts).toEqual([LANDED13]);
  expect(ran.filter((r) => r === READ)).toEqual([]);
});

test("a reload after a turn starts no turn for what sluice has handed over", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  inbox.answers = [printing(""), printing(LANDED13)];
  await watch.print(queued13());
  await watch.print(landed13());
  expect(seen.prompts).toEqual([LANDED13]);
  // The first overview collects what is owed; the landing, what is new.
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW]);
  // The agent confirmed #13; reloaded, the next watch prints the same, a
  // first overview, whose collect asks for everything still owed and is
  // told nothing.
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await watch.print(landed13());
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW, INBOX]);
  expect(seen.prompts).toEqual([LANDED13]);
  expect(seen.toasts).toEqual(["sluice: #13 landed on main"]);
});

test("a reload after a turn the agent did not confirm starts it again", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // Nobody confirmed #13's blame, so the reload's collect prints it again.
  inbox.answers = [printing(""), printing(BLAME13), printing(BLAME13)];
  await watch.print(overview());
  await watch.print(blamed());
  expect(seen.prompts).toEqual([BLAME13]);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await watch.print(blamed());
  expect(inbox.ran).toEqual([INBOX, INBOX_NEW, INBOX]);
  expect(seen.prompts).toEqual([BLAME13, BLAME13]);
  // The blame is not news again: one toast, from before the reload.
  expect(seen.toasts).toEqual(["sluice: #13 was blamed"]);
});

test("two outcomes while a collect runs give one more collect once it ends", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // #12, #13 and #14 are all the session's.
  const ours = () => {
    const d = queued13();
    for (const s of d.branches[0]!.submissions) s.from.path = ROOT;
    return d;
  };
  const recent = (d: Overview, lines: Recent[]) => {
    d.branches[0]!.recent.push(...lines);
    return d;
  };
  const landed: Recent = {
    at: "2026-10-16T19:46:00Z",
    event: "landed",
    ids: [13],
    to: "5d6e7f8a9b",
  };
  const landed12: Recent = { at: "2026-10-16T19:47:00Z", event: "landed", ids: [12], to: "6e7f8" };
  const blamed14: Recent = { at: "2026-10-16T19:48:00Z", event: "blamed", id: 14 };
  await watch.print(ours());
  expect(inbox.ranIn).toHaveLength(1);
  // #13 lands, and sluice is slow to answer the collect.
  let answer!: () => void;
  inbox.held = new Promise<void>((done) => {
    answer = done;
  });
  await watch.print(recent(ours(), [landed]));
  expect(inbox.ranIn).toHaveLength(2);
  await watch.print(recent(ours(), [landed, landed12]));
  await watch.print(recent(ours(), [landed, landed12, blamed14]));
  expect(inbox.ranIn).toHaveLength(2);
  expect(seen.toasts).toEqual([
    "sluice: #13 landed on main",
    "sluice: #12 landed on main",
    "sluice: #14 was blamed",
  ]);
  inbox.held = null;
  answer();
  await clock.settle();
  expect(inbox.ranIn).toHaveLength(3);
});

test("a watch that can't be started is tried 1, 2, 4, 8, 16 and 32 s apart, then every minute, and nothing read between", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  // The session's watch prints the queue and ends, and none starts after it.
  await watch.print(justSetUp());
  await watch.end();
  watch.failing = true;
  for (const gap of [1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000])
    await startedAgainAfter(clock, watch, gap);
  expect(watch.started.slice(1).map((t, i) => t - watch.started[i]!)).toEqual([
    1000, 2000, 4000, 8000, 16000, 32000, 60000, 60000,
  ]);
  expect(ran.filter((r) => r === READ)).toEqual([]);
  const band = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "AbovePrompt",
    props: BAND,
  });
  expect(
    await band.find({ type: "Text", text: "sluice main · 1 submission · #3 waiting" }),
  ).toBeDefined();
  await band.unmount();
});

test("a watch killed after ten minutes is started again a second later", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await clock.advance(600_000);
  await watch.end();
  await startedAgainAfter(clock, watch, 1000);
  expect(watch.started).toEqual([NOW, NOW + 601_000]);
});

test("a watch open ten minutes after the waits grew to a minute is started again a second later", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  watch.failing = true;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  for (const gap of [1000, 2000, 4000, 8000, 16000, 32000])
    await startedAgainAfter(clock, watch, gap);
  // The next wait is a minute, and that watch stays open.
  watch.failing = false;
  await startedAgainAfter(clock, watch, 60_000);
  expect(watch.isOpen()).toBe(true);
  await clock.advance(600_000);
  await watch.end();
  await startedAgainAfter(clock, watch, 1000);
});

test("/sluice twice with the watch open leaves one watch running", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(overview());
  await $.command.run(slash(true));
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toHaveLength(1);
  expect(watch.isOpen()).toBe(true);
  // No other is started until it ends.
  await clock.advance(60_000);
  expect(watch.started).toHaveLength(1);
  await watch.end();
  await startedAgainAfter(clock, watch, 1000);
});

/** The overview a watch ends on where the repository has no sluice (129). */
function noSluice(): Overview {
  return { ...overview(), branches: [], sessions: [], previews: [], set_up: false };
}

// One subscription for the session (the change
// `the-plugins-hold-one-subscription-each`, D6): at most one watch runs at
// any time over the session's life.

test("the board's watch serves the session's folder with no --stdin", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(watch.spawned).toEqual([{ argv: "sluice overview --json --watch", cwd: ROOT }]);
});

test("a watch that ends three times never runs beside the next", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(overview());
  for (let i = 0; i < 3; i++) {
    // Each has run ten minutes, so the next follows a second later.
    await clock.advance(600_000);
    await watch.end();
    await startedAgainAfter(clock, watch, 1000);
    await watch.print(overview());
  }
  expect(watch.started).toHaveLength(4);
  expect(watch.most).toBe(1);
});

test("/sluice twice where the repository has no sluice starts one watch", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(noSluice());
  await watch.exit(0, "");
  await $.command.run(slash(true));
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toHaveLength(2);
  expect(watch.isOpen()).toBe(true);
  expect(watch.most).toBe(1);
});

test("a second session start in one load leaves one watch", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const watch = watchedHost(on, clock, nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(overview());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  expect(watch.started).toHaveLength(1);
  expect(watch.most).toBe(1);
});

const NOT_SET_UP = [
  "sluice isn't set up in this repository.",
  "Ask Claude to set sluice up in this repository.",
];

test("a session where sluice isn't set up shows nothing, and starts no watch again", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(noSluice());
  await watch.exit(0, "");
  expect(await bandOf($)).toEqual([]);
  expect(seen.status.at(-1)).toBeUndefined();
  // Ten minutes on: no other watch, and nothing run in its place.
  await clock.advance(600_000);
  expect(watch.started).toEqual([NOW]);
  expect(ran).toEqual([]);
  expect(seen.toasts).toEqual([]);
  expect(seen.prompts).toEqual([]);
});

test("a queue the board showed gives way to a watch saying sluice isn't set up", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(justSetUp());
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  expect(seen.status.at(-1)).toBe("sluice: 1 submission");
  await watch.end();
  await startedAgainAfter(clock, watch, 1000);
  await watch.print(noSluice());
  await watch.exit(0, "");
  expect(await bandOf($)).toEqual([]);
  expect(seen.status.at(-1)).toBeUndefined();
  expect((await paneTexts($)).join("\n")).not.toContain("main");
  await clock.advance(600_000);
  expect(watch.started).toHaveLength(2);
});

test("still not set up: /sluice reads once more, and the pane says so and what to do", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(noSluice());
  await watch.exit(0, "");
  await $.command.run(slash(true));
  await clock.settle();
  // Started at once, and the pane reads while it does.
  expect(watch.started).toEqual([NOW, NOW]);
  expect(await paneTexts($)).toEqual(["Reading sluice…"]);
  await watch.print(noSluice());
  await watch.exit(0, "");
  expect(await paneTexts($)).toEqual(NOT_SET_UP);
  expect(await bandOf($)).toEqual([]);
  expect(seen.status.at(-1)).toBeUndefined();
  // No other watch until the next /sluice, which closes the pane, then opens it.
  await clock.advance(600_000);
  expect(watch.started).toHaveLength(2);
  await $.command.run(slash(true));
  await $.command.run(slash(true));
  await clock.settle();
  expect(watch.started).toEqual([NOW, NOW, NOW + 600_000]);
  expect(ran.filter((r) => r.startsWith("sluice"))).toEqual([]);
});

test("the first overview after the repository had no sluice collects everything owed, with no toast", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  inbox.answers = [printing(""), printing(LANDED13)];
  await watch.print(queued13());
  expect(inbox.ran).toEqual([INBOX]);
  await watch.print(noSluice());
  await watch.exit(0, "");
  // Set up again: /sluice's watch prints #13 landed meanwhile, a first
  // overview, which collects all that is owed and raises no toast for it.
  await $.command.run(slash(true));
  await clock.settle();
  await watch.print(landed13());
  expect(inbox.ran).toEqual([INBOX, INBOX]);
  expect(inbox.ranIn).toEqual([ROOT, ROOT]);
  expect(seen.prompts).toEqual([LANDED13]);
  expect(seen.toasts).toEqual([]);
});

test("set up during the session: /sluice's watch shows the queue, and is followed", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  const watch = watchedHost(on, clock, seen);
  panes(on);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(noSluice());
  await watch.exit(0, "");
  await $.command.run(slash(true));
  await clock.settle();
  await watch.print(justSetUp());
  const pane = await mountPane($);
  const heads = await pane.findAll({ type: "Text", text: /^(TESTING|QUEUED|RECENT)$/ });
  expect(heads.map((h) => h.text)).toEqual(["QUEUED"]);
  expect(await pane.find({ type: "Text", text: /#3 queued\s+w3 claude/ })).toBeDefined();
  await pane.unmount();
  expect(await bandOf($)).toEqual(["sluice main · 1 submission · #3 waiting"]);
  expect(seen.status.at(-1)).toBe("sluice: 1 submission");
  // Followed from now: the next watch after the wait, unasked.
  await watch.end();
  await startedAgainAfter(clock, watch, 1000);
});

test("a failure opens in place, folds, and moves no one", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  host(on, [blamedWithLog(LOG)], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  ran.length = 0;
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  // The blamed submission: its test, its log and the log's last lines.
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: "last line" })).toBeDefined();
  expect(await pane.find({ type: "Text", text: `${LOG} [copy]` })).toBeDefined();
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: "last line" })).toBeUndefined();
  // Nothing but sluice's own record and the log was read: no pane moved.
  expect(ran).toEqual(["sluice status 9 --branch main --json", `tail -n 20 -- ${LOG}`]);
  await pane.unmount();
});

test("a log it cannot read says why", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = {
    prompts: [] as string[],
    toasts: [] as string[],
    status: [] as (string | undefined)[],
  };
  host(on, [blamedWithLog("/gone/cli:test.log")], seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  await pane.press({ key: "log-main-cli:test" });
  await pane.redraw();
  expect(
    await pane.find({ type: "Text", text: /cannot read it: tail: No such file or directory/ }),
  ).toBeDefined();
  await pane.unmount();
});

const THIS_RUN = "this run: passed 19:42 on devbox";
const THAT_RUN = "the run it contradicts: failed 19:10 on mac-studio · main 4f1c2aa";

/** The texts an opened row shows, from its test's name on. */
async function openedTexts(pane: {
  findAll: (q: { type: string }) => Promise<{ text: string }[]>;
}) {
  const all = (await pane.findAll({ type: "Text" })).map((t) => t.text);
  return all.slice(all.indexOf("cli:test"));
}

test("a flaky failure opens to this run and the run it contradicts, each with its log", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const status = { id: 9, failed: [{ task: "cli:test", log: LOG, flaky: flaky(OTHER) }] };
  host(
    on,
    [blamedWithLog(LOG, flaky(OTHER))],
    nothingSeen(),
    {},
    {
      "sluice status 9 --branch main --json": JSON.stringify(status),
      [`tail -n 20 -- ${OTHER}`]: "other first\nother last\n",
    },
  );
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  ran.length = 0;
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  const both = [
    "cli:test",
    THIS_RUN,
    `${LOG} [copy]`,
    "first",
    "last line",
    THAT_RUN,
    `${OTHER} [copy]`,
    "other first",
    "other last",
  ];
  // The blamed row, from sluice's record of #9.
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  expect((await openedTexts(pane)).slice(0, both.length)).toEqual(both);
  await pane.press({ key: "blamed-main-9-2026-10-16T19:42:00Z" });
  await pane.redraw();
  // The `log of` row, from the combination the overview lists.
  await pane.press({ key: "log-main-cli:test" });
  await pane.redraw();
  expect((await openedTexts(pane)).slice(0, both.length)).toEqual(both);
  expect(ran).toEqual([
    "sluice status 9 --branch main --json",
    `tail -n 20 -- ${LOG}`,
    `tail -n 20 -- ${OTHER}`,
    `tail -n 20 -- ${LOG}`,
    `tail -n 20 -- ${OTHER}`,
  ]);
  await pane.unmount();
});

test("a flaky failure whose contradicting run's log was not kept says so", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  host(on, [blamedWithLog(LOG, flaky())], nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  ran.length = 0;
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  await pane.press({ key: "log-main-cli:test" });
  await pane.redraw();
  const shown = await openedTexts(pane);
  expect(shown.slice(shown.indexOf(THAT_RUN), shown.indexOf(THAT_RUN) + 2)).toEqual([
    THAT_RUN,
    "its log was not kept",
  ]);
  expect(ran).toEqual([`tail -n 20 -- ${LOG}`]);
  await pane.unmount();
});

test("a flaky failure whose own log cannot be read still shows the run it contradicts", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const gone = "/gone/cli:test.log";
  host(
    on,
    [blamedWithLog(gone, flaky(OTHER))],
    nothingSeen(),
    {},
    {
      [`tail -n 20 -- ${OTHER}`]: "other first\nother last\n",
    },
  );
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  await pane.press({ key: "log-main-cli:test" });
  await pane.redraw();
  expect((await openedTexts(pane)).slice(0, 8)).toEqual([
    "cli:test",
    THIS_RUN,
    `${gone} [copy]`,
    "cannot read it: tail: No such file or directory",
    THAT_RUN,
    `${OTHER} [copy]`,
    "other first",
    "other last",
  ]);
  await pane.unmount();
});

/** A RECENT conflict, its `with` as given: closed, opened to `inWay`, folded, with no command run. */
function conflictOpens(with_: number[], inWay: string): TestBody {
  return async ($, on) => {
    const next = "next: take main into the worktree, resolve and commit, then sluice submit main";
    const clock = mock.clock(on);
    await clock.set(NOW);
    host(on, [withConflict(with_)], { prompts: [], toasts: [], status: [] });
    await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
    await clock.settle();
    ran.length = 0;
    const pane = await $.ui.mount({
      plugin: "sluice-board",
      surface: "terminal",
      component: "Pane",
      props: PANE,
      requestId: "sluice",
    });
    const key = "conflict-main-218-2026-10-16T19:44:00Z";
    expect(
      await pane.find({ type: "Text", text: /⚠ 19:44 #218 conflict · docs\/builds\.md/ }),
    ).toBeDefined();
    expect(await pane.find({ type: "Text", text: /^files: / })).toBeUndefined();
    await pane.press({ key });
    await pane.redraw();
    expect(await pane.find({ type: "Text", text: "files: docs/builds.md" })).toBeDefined();
    expect(await pane.find({ type: "Text", text: inWay })).toBeDefined();
    expect(await pane.find({ type: "Text", text: next })).toBeDefined();
    await pane.press({ key });
    await pane.redraw();
    expect(await pane.find({ type: "Text", text: /^(files|in the way|next): / })).toBeUndefined();
    // Built from the line and the overview: no command ran.
    expect(ran).toEqual([]);
    await pane.unmount();
  };
}

test(
  "a RECENT conflict with a queued submission opens to it, and folds",
  conflictOpens([217], "in the way: #217 (feature/runner)"),
);

test(
  "a RECENT conflict with what the branch landed opens to it, and folds",
  conflictOpens([], "in the way: what main has landed"),
);

/** What a forecast host answers, changed as a test goes; `doc` is its watch's first overview. */
type World = { doc: Overview; head: string; answer: object; forecasts: number };

/** The fixture with #13, this session's, gone from main's queue. */
function free(at = "9ecc34a1b2"): Overview {
  const d = overview();
  d.branches[0]!.at = at;
  d.branches[0]!.submissions = d.branches[0]!.submissions.filter((s) => s.id !== 13);
  return d;
}

const CLEAR = { conflict: null, head: "h1", onto: "9ecc34a1b2", ahead: [12, 14] };
const LANDED = { conflict: { paths: ["docs/builds.md"], with: [] } };
const WITH_217 = {
  conflict: {
    paths: ["docs/builds.md"],
    with: [{ id: 217, from: { agent: "runner", worktree: "feature/runner" } }],
  },
};

/**
 * A host whose watch the test hands each document to, `w.doc` first; whose
 * worktree is 4 commits ahead at `w.head`, whose forecast says `w.answer`,
 * and whose `sluice inbox` prints nothing.
 */
function forecastHost(
  on: On,
  clock: MockClock,
  w: World,
  seen: { prompts: string[]; toasts: string[]; status: (string | undefined)[] },
) {
  inboxAfresh();
  on("session.start", ($, e) => ({ cwd: e.cwd }));
  on("command.register", ($, e) => ({ value: { command: e.name } }));
  on("process.run", ($, e) => {
    const argv = e.argv.join(" ");
    const out = (stdout: string) => ({ value: { exitCode: 0, stdout, stderr: "", ...WHOLE } });
    if (argv === INBOX || argv === INBOX_NEW) return inboxRun(e.init?.cwd, argv);
    if (argv === "git rev-parse --show-toplevel") return out(`${ROOT}\n`);
    if (argv === "git rev-parse HEAD") return out(`${w.head}\n`);
    if (argv.startsWith("git rev-list --count ")) return out("4\n");
    if (argv === "sluice submit main --check --json") {
      w.forecasts += 1;
      return out(JSON.stringify(w.answer));
    }
    return { value: { exitCode: 1, stdout: "", stderr: `no ${argv}`, ...WHOLE } };
  });
  on("ui.status", ($, e) => {
    seen.status.push(e.text);
    return { value: undefined };
  });
  on("ui.toast", ($, e) => {
    seen.toasts.push(e.text);
    return { value: undefined };
  });
  on("ui.notify", () => ({ value: { isSent: true as const, channel: "terminal_bell" } }));
  on("prompt.submit", ($, e) => {
    seen.prompts.push(e.text);
    return { text: e.text };
  });
  return watches(on, clock);
}

function nothingSeen() {
  return { prompts: [] as string[], toasts: [] as string[], status: [] as (string | undefined)[] };
}

test("a press only reads", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  // One pane with every kind of row that opens: main testing c41 with its
  // tasks, the agents' checks, a RECENT conflict and #9's flaky blame with
  // its `log of` row; and this session's work forecast to conflict.
  const d = withConflict([217]);
  const main = d.branches[0]!;
  main.candidates[0]!.tasks = withTasks().branches[0]!.candidates[0]!.tasks;
  main.submissions = main.submissions.filter((s) => s.id !== 13);
  main.candidates.push(blamedWithLog(LOG, flaky(OTHER)).branches[0]!.candidates.at(-1)!);
  const checks = withChecks();
  d.sessions = checks.sessions;
  d.room = checks.room;
  const status = { id: 9, failed: [{ task: "cli:test", log: LOG, flaky: flaky(OTHER) }] };
  host(
    on,
    [d],
    nothingSeen(),
    {},
    {
      "sluice status 9 --branch main --json": JSON.stringify(status),
      [`tail -n 20 -- ${OTHER}`]: "other first\nother last\n",
      "git rev-parse HEAD": "h1\n",
      "git rev-list --count 9ecc34a1b2..HEAD": "4\n",
      "sluice submit main --check --json": JSON.stringify(WITH_217),
    },
  );
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  ran.length = 0;
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  expect(
    await pane.find({ type: "Text", text: "  ⚠ your work would conflict · docs/builds.md" }),
  ).toBeDefined();
  const rows = [
    "tasks-main-c41",
    "checks-/work/sluice/fix-cards",
    "checks-/work/sluice/e2e-stack",
    "checks-/work/sluice/done-ok",
    "conflict-main-218-2026-10-16T19:44:00Z",
    "forecast-main",
    "blamed-main-9-2026-10-16T19:42:00Z",
    "log-main-cli:test",
  ];
  for (const key of rows) {
    // Open, then fold.
    for (let n = 0; n < 2; n++) {
      await pane.press({ key });
      await pane.redraw();
    }
  }
  // Only sluice's record of the blamed submission and the logs' last lines
  // were read, by the blame and the `log of` row; the rest ran nothing.
  expect(ran).toEqual([
    "sluice status 9 --branch main --json",
    `tail -n 20 -- ${LOG}`,
    `tail -n 20 -- ${OTHER}`,
    `tail -n 20 -- ${LOG}`,
    `tail -n 20 -- ${OTHER}`,
  ]);
  await pane.unmount();
});

test("main landing a change to the same lines starts one turn, once, a reload included", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const w: World = { doc: free(), head: "h1", answer: CLEAR, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  expect(w.forecasts).toBe(1);
  expect(seen.prompts).toEqual([]);

  w.answer = LANDED;
  await watch.print(free("b2c3d4e5f6"));
  expect(w.forecasts).toBe(2);
  expect(seen.prompts).toEqual([
    "[sluice] Your work would conflict with what main has landed, in docs/builds.md. " +
      "Merge main into your worktree now and resolve it while it is small (check with `sluice submit --check main`).",
  ]);
  expect(seen.toasts).toContain("sluice: your work would conflict with main in docs/builds.md");

  // Nothing it rests on moved: no forecast.
  await watch.print(free("b2c3d4e5f6"));
  expect(w.forecasts).toBe(2);

  // A reload tells nothing again.
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await watch.print(free("b2c3d4e5f6"));
  expect(seen.prompts).toHaveLength(1);
});

test("a conflict with a queued submission names it, and its failing clears it once", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  // #220, this session's, waits on main: no forecast while it does.
  const d0 = free();
  d0.branches[0]!.submissions.push({
    id: 220,
    state: "queued",
    from: { agent: "claude", worktree: "design-runner", path: ROOT },
  });
  const w: World = { doc: d0, head: "h1", answer: CLEAR, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  expect(w.forecasts).toBe(0);

  // #220 is removed for a conflict with #217, queued ahead of it.
  const d1 = free();
  d1.branches[0]!.submissions.push({
    id: 217,
    state: "testing",
    from: { agent: "runner", worktree: "feature/runner" },
  });
  d1.branches[0]!.recent.push({
    schema: "sluice/log@1",
    at: "2026-10-16T19:45:00Z",
    event: "conflict",
    id: 220,
    paths: ["docs/builds.md"],
  });
  w.answer = WITH_217;
  await watch.print(d1);
  expect(
    seen.prompts.some((p) => p.includes("with #217 (feature/runner), in docs/builds.md")),
  ).toBe(true);
  const told = seen.prompts.length;

  // #217 fails; the head has not moved.
  const d2 = free();
  d2.branches[0]!.recent = d1.branches[0]!.recent;
  w.answer = CLEAR;
  await watch.print(d2);
  expect(seen.prompts.slice(told)).toEqual([
    "[sluice] Your work no longer conflicts with main or anything queued. #220 can be submitted again: `sluice submit main`.",
  ]);
  await watch.print(d2);
  expect(seen.prompts).toHaveLength(told + 1);
});

test("the forecast's conflict is a row under its branch, kept across a reload until none is found", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const d1 = free();
  d1.branches[0]!.submissions.push({
    id: 217,
    state: "testing",
    from: { agent: "runner", worktree: "feature/runner" },
  });
  const w: World = { doc: d1, head: "h1", answer: WITH_217, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  expect(w.forecasts).toBe(1);
  const row = "  ⚠ your work would conflict · docs/builds.md";
  const mount = () =>
    $.ui.mount({
      plugin: "sluice-board",
      surface: "terminal",
      component: "Pane",
      props: PANE,
      requestId: "sluice",
    });
  let pane = await mount();
  expect(await pane.find({ type: "Text", text: row })).toBeDefined();
  // Under main, after its queue and before RECENT.
  const order = (
    await pane.findAll({ type: "Text", text: /^(QUEUED|RECENT| {2}⚠ your work)/ })
  ).map((t) => t.text);
  expect(order).toEqual(["QUEUED", row, "RECENT"]);
  await pane.press({ key: "forecast-main" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: "files: docs/builds.md" })).toBeDefined();
  expect(
    await pane.find({ type: "Text", text: "in the way: #217 (feature/runner)" }),
  ).toBeDefined();
  expect(
    await pane.find({
      type: "Text",
      text: "next: take main into the worktree, resolve and commit, then sluice submit main",
    }),
  ).toBeDefined();
  await pane.press({ key: "forecast-main" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: /^in the way: / })).toBeUndefined();
  await pane.unmount();

  // A reload keeps it.
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await watch.print(d1);
  pane = await mount();
  expect(await pane.find({ type: "Text", text: row })).toBeDefined();
  await pane.unmount();

  // #217 fails: the next forecast finds no conflict, and the row goes.
  w.answer = CLEAR;
  await watch.print(free());
  pane = await mount();
  expect(await pane.find({ type: "Text", text: /your work would conflict/ })).toBeUndefined();
  await pane.unmount();
});

test("a conflict cleared by the agent's own merge starts no turn", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const w: World = { doc: free(), head: "h1", answer: CLEAR, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  w.answer = LANDED;
  await watch.print(free("b2c3d4e5f6"));
  expect(seen.prompts).toHaveLength(1);

  w.head = "h2";
  w.answer = CLEAR;
  await watch.print(free("b2c3d4e5f6"));
  expect(w.forecasts).toBe(3);
  expect(seen.prompts).toHaveLength(1);
});

test("a conflict there when the session starts is a toast, not a turn", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const w: World = { doc: free(), head: "h1", answer: LANDED, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  expect(w.forecasts).toBe(1);
  expect(seen.prompts).toEqual([]);
  expect(seen.toasts).toContain("sluice: your work would conflict with main in docs/builds.md");
});

test("a forecast conflict with no paths and a session state with no ids draw as empty", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  // Every write of the session's state loses its ids.
  on("state.set", { plugin: "sluice-board", key: "mine" }, ($, e, next) => {
    const { ids: _, ...rest } = e.value;
    return next({ ...e, value: rest as Mine });
  });
  const w: World = { doc: free(), head: "h1", answer: { conflict: { with: [] } }, forecasts: 0 };
  const seen = nothingSeen();
  const watch = forecastHost(on, clock, w, seen);
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  expect(w.forecasts).toBe(1);
  expect(seen.toasts).toContain("sluice: your work would conflict with main in a file");
  const pane = await $.ui.mount({
    plugin: "sluice-board",
    surface: "terminal",
    component: "Pane",
    props: PANE,
    requestId: "sluice",
  });
  expect(await pane.find({ type: "Text", text: /#12 testing/ })).toBeDefined();
  expect(
    await pane.find({ type: "Text", text: "  ⚠ your work would conflict · a file" }),
  ).toBeDefined();
  await pane.press({ key: "forecast-main" });
  await pane.redraw();
  expect(await pane.find({ type: "Text", text: "files: " })).toBeDefined();
  await pane.unmount();
});

test("SLUICE_FORECAST=0 turns forecasting off", async ($, on) => {
  const clock = mock.clock(on);
  await clock.set(NOW);
  mock.env(on, { SLUICE_FORECAST: "0" });
  const w: World = { doc: free(), head: "h1", answer: LANDED, forecasts: 0 };
  const watch = forecastHost(on, clock, w, nothingSeen());
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(w.doc);
  await watch.print(free("b2c3d4e5f6"));
  expect(w.forecasts).toBe(0);
});

test(
  "the forecast option turns forecasting off",
  { options: { forecast: false } },
  async ($, on) => {
    const clock = mock.clock(on);
    await clock.set(NOW);
    const w: World = { doc: free(), head: "h1", answer: LANDED, forecasts: 0 };
    const watch = forecastHost(on, clock, w, nothingSeen());
    await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
    await clock.settle();
    await watch.print(w.doc);
    await watch.print(free("b2c3d4e5f6"));
    expect(w.forecasts).toBe(0);
  },
);

/** The releases `.claude-plugin/sluice.json` names, as the board reads them. */
const RANGE = JSON.stringify({ from: "0.55.0", before: "1.0.0" });

/** The board's manifest of the sluice it runs with reads `range`. */
function manifest(on: On, range: string = RANGE) {
  on("fs.read", ($, e, next) =>
    e.path.endsWith("/.claude-plugin/sluice.json") ? { value: range } : next(e),
  );
}

const TOO_OLD =
  "the sluice board needs sluice 0.55.0 or later, which has `sluice inbox`, and this host's is sluice 0.44.0: it shows nothing and delivers nothing until sluice is upgraded";
const TOO_NEW =
  "the sluice board runs with sluice before 1.0.0, and this host's is sluice 1.2.0: it shows nothing and delivers nothing until the board is updated";

/**
 * A board on a host whose sluice prints `version`: an outcome of the
 * session's own is pending, and `sluice inbox` would print it.
 */
async function startedOn($: Engine, on: On, version: string) {
  const clock = mock.clock(on);
  await clock.set(NOW);
  const seen = nothingSeen();
  manifest(on);
  const watch = watchedHost(on, clock, seen, { [VERSION]: `${version}\n` });
  inbox.answers = [printing(BLAME13)];
  panes(on);
  ran.length = 0;
  await $.session.start({ cwd: ROOT, surface: "terminal", isInteractive: true });
  await clock.settle();
  await watch.print(blamed());
  return { clock, seen, watch };
}

for (const [version, words] of [
  ["sluice 0.44.0", TOO_OLD],
  ["sluice 1.2.0", TOO_NEW],
] as const) {
  test(`${version} on the host: the board says so once, and shows and delivers nothing`, async ($, on) => {
    const { clock, seen, watch } = await startedOn($, on, version);
    // Said once, in a toast; nothing collected, no turn, nothing drawn.
    expect(seen.toasts).toEqual([words]);
    expect(ran.filter((r) => r === VERSION)).toEqual([VERSION]);
    expect(ran).not.toContain(INBOX);
    expect(seen.prompts).toEqual([]);
    expect(await bandOf($)).toEqual([]);
    // The watch is ended, and none follows it.
    expect(watch.isOpen()).toBe(false);
    await clock.advance(120_000);
    expect(watch.started).toHaveLength(1);
    // /sluice answers with the same words, and opens no pane.
    expect((await $.command.run(slash(true))).text).toBe(words);
    expect(watch.started).toHaveLength(1);
    expect(seen.toasts).toEqual([words]);
  });
}

test("a sluice the board runs with: it collects the session's outcome as before", async ($, on) => {
  const { seen } = await startedOn($, on, "sluice 0.56.0");
  expect(seen.toasts).toEqual([]);
  expect(ran).toContain(INBOX);
  expect(seen.prompts).toEqual([BLAME13]);
});

test("a build that names no release: the board runs", async ($, on) => {
  const { seen } = await startedOn($, on, "sluice 0.0.0");
  expect(seen.toasts).toEqual([]);
  expect(seen.prompts).toEqual([BLAME13]);
});
