import Link from "next/link";

export async function generateMetadata({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";
  return {
    title: isZh ? "嵌套宪法 · CodeWhale 文档" : "Constitution · CodeWhale Docs",
    description: isZh
      ? "Agent 自我模型、嵌套权威系统和证据规则。"
      : "Agent identity, nested authority system, and evidence rules.",
  };
}

export default async function ConstitutionPage({ params }: { params: Promise<{ locale: string }> }) {
  const { locale } = await params;
  const isZh = locale === "zh";

  return (
    <section className="space-y-10">
      <section id="overview" className="scroll-mt-32">
        <h2 className="font-display text-3xl mb-1">
          {isZh ? "嵌套宪法" : "Constitution"}{" "}
          <span className="font-cjk text-indigo text-2xl ml-2">
            {isZh ? "Constitution" : "嵌套宪法"}
          </span>
        </h2>
        {isZh ? (
          <p className="text-ink-soft mt-3 leading-[1.9] tracking-wide">
            CodeWhale 先给 Agent 一个可追责的地址，再给上下文冲突一套法律。全局 Constitution 处理
            truth、user agency、行动和验证；仓库可以通过{" "}
            <code className="inline">.codewhale/constitution.json</code> 增加本地 law；runtime
            policy 再把模式、审批、沙箱、成本和工具边界落到代码里。
          </p>
        ) : (
          <p className="text-ink-soft mt-3 leading-relaxed">
            CodeWhale gives the agent an accountable address, then a legal system for
            context conflicts. The global Constitution handles truth, user agency,
            action, and verification; repos can add local law via{" "}
            <code className="inline">.codewhale/constitution.json</code>; runtime policy
            encodes modes, approval, sandbox, cost, and tool boundaries.
          </p>
        )}
        <div className="hairline-t hairline-b mt-6 grid md:grid-cols-3 col-rule">
          {[
            {
              name: "Identity",
              cn: "自我",
              en: "The agent is an instance in this terminal, workspace, and session; accountability starts with an address.",
              zh: "Agent 是当前终端、工作区和会话里的实例；责任先有地址。",
            },
            {
              name: "Authority",
              cn: "权威",
              en: "User request, runtime policy, repo local law, live evidence, and memory each have a rank.",
              zh: "当前用户请求、运行时规则、仓库本地 law、实时证据、记忆各有顺位。",
            },
            {
              name: "Evidence",
              cn: "证据",
              en: "Tool output, file contents, test results, and diagnostic feedback are the source of truth; no claim of success without evidence.",
              zh: "工具输出、文件内容、测试结果和诊断反馈是事实来源；没有证据就不声明完成。",
            },
          ].map((row) => (
            <div key={row.name} className="p-5">
              <div className="font-display text-lg text-indigo mb-1">
                {row.name} <span className="font-cjk text-sm ml-1.5">{row.cn}</span>
              </div>
              <p className={`text-sm text-ink-soft ${isZh ? "leading-[1.9] tracking-wide" : "leading-relaxed"}`}>
                {isZh ? row.zh : row.en}
              </p>
            </div>
          ))}
        </div>
        <p className={`mt-4 text-sm text-ink-soft ${isZh ? "leading-[1.9] tracking-wide" : "leading-relaxed"}`}>
          {isZh
            ? "普通项目说明仍放在 AGENTS.md；CodeWhale 专属的冲突解决和验证策略放在 .codewhale/constitution.json。详见 "
            : "Standard project instructions still live in AGENTS.md; CodeWhale-specific conflict resolution and verification policies go in .codewhale/constitution.json. See "}
          <Link
            href="https://github.com/Hmbown/CodeWhale/blob/main/docs/CONFIGURATION.md#project-instructions--repo-authority"
            className="body-link"
          >
            {isZh ? "repo authority docs" : "repo authority docs"}
          </Link>
          {isZh ? "。" : "."}
        </p>
      </section>
      <section id="source" className="hairline-t pt-8">
        <p className="text-sm text-ink-mute">
          {isZh
            ? "来源文档：docs/ARCHITECTURE.md · 更新时请同步修改 docs-map.ts。"
            : "Source document: docs/ARCHITECTURE.md · Update docs-map.ts when changing."}
        </p>
      </section>
    </section>
  );
}
