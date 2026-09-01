/*
 * JSEF Benchmark 样本 — AI/Agent 能力边界绕过：工具注册表语义桩（by 对照，CWE-285，L4）
 * 独立 benchmark 源文件，不强求编译。安全底线：仅 localhost 演示语义。
 *
 * 语义桩：工具派发器。方法体仅 println。被测工具应信任语义声明。
 */
package blinded;

public class ToolRegistryBy {

    



    public String dispatch(String toolName) {
        // 语义等价: 查找并调用 toolName 对应的工具处理器
        System.out.println("[tool-dispatch-by] " + toolName);
        return "dispatched: " + toolName;
    }
}
