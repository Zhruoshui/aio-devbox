import { validateSource, validateToml, findDuplicateJsonKeys } from "/home/ruoshui/Documents/MyProject/agent/sandboxes/aio/mgr-web/src/components/SourceCodeEditor";
import { parseIncrementalFiles } from "/home/ruoshui/Documents/MyProject/agent/sandboxes/aio/mgr-web/src/pages/models/sourceFiles";

let fails = 0;
const check = (name: string, cond: boolean, extra?: unknown) => {
  if (!cond) { fails++; console.error("FAIL", name, extra ?? ""); } else console.log("ok  ", name);
};

// ── TOML checker
const goodToml = `# comment
model = "glm-4.6"
model_provider = "bigmodel"

[model_providers.bigmodel]
name = "BigModel"
base_url = "https://open.bigmodel.cn/api/coding/paas/v4"
env_key = "BIGMODEL_API_KEY"
wire_api = "chat"

[tags]
multi = [
  "a",
  "b",
]
same_line_ml = """hello
world"""
literal = 'no \\escape'
inline = { a = 1, b = 2 }
bracket_in_str = "array[0] # not a comment"
`;
check("toml good passes", validateToml(goodToml) === "", validateToml(goodToml));
check("toml missing =", validateToml('model "x"').includes("expected"), validateToml('model "x"'));
check("toml unterminated str", validateToml('a = "abc').includes("unterminated"), validateToml('a = "abc'));
check("toml unclosed array", validateToml("a = [\n 1,\n").includes("unclosed"), validateToml("a = [\n 1,\n"));
check("toml unbalanced close", validateToml("a = 1]\n").includes("unbalanced"), validateToml("a = 1]\n"));
check("toml bracket in string ok", validateToml('k = "foo [bar"').includes("unclosed") === false, validateToml('k = "foo [bar"'));
check("toml multiline same-line close", validateToml('k = """abc"""').includes("unterminated") === false, validateToml('k = """abc"""'));
check("toml multiline unterminated", validateToml('k = """abc\nstill').includes("multi-line"), validateToml('k = """abc\nstill'));
check("toml comment with hash", validateToml('a = 1 # trailing # comment').includes("expected") === false, validateToml('a = 1 # trailing # comment'));

// ── duplicate keys
check("dup key detected", findDuplicateJsonKeys('{"a":1,"a":2}') === '"a"', findDuplicateJsonKeys('{"a":1,"a":2}'));
check("dup nested detected", findDuplicateJsonKeys('{"a":{"b":1,"b":2}}') === '"b"');
check("same key diff objects ok", findDuplicateJsonKeys('{"a":{"x":1},"b":{"x":2}}') === null);
check("key in string value ok", findDuplicateJsonKeys('{"a":"b:\\"c"}') === null);
check("validateSource json dup blocks", validateSource('{"a":1,"a":2}', "json").includes("duplicate"));
check("validateSource jsonc dup blocks", validateSource('{\n// c\n"a":1,\n"a":2}', "jsonc").includes("duplicate"));

// ── URL shape in parse-back
const bad = parseIncrementalFiles("pi", JSON.stringify({ providers: { x: { baseUrl: "htp:/x", api: "openai-completions" } } }), "{}");
check("bad url rejected", bad.error?.includes("http(s)"), bad.error);
const good = parseIncrementalFiles("pi", JSON.stringify({ providers: { x: { baseUrl: "https://x.com/v1", api: "openai-completions" } } }), "{}");
check("good url passes", good.error === undefined, good.error);

// ── regression: normal files still validate clean
check("pi round-trip still clean", validateSource(JSON.stringify({ providers: {} }), "json") === "");
check("empty toml still empty", validateSource("", "toml") === "empty");

process.exit(fails === 0 ? 0 : 1);
