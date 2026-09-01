/*
 * JSEF Benchmark 真假混淆样本 — 命令注入（B5，CWE-78）
 * BX 版：先经允许的字符集校验，并使用数组参数化 exec（无 shell 解释），用户输入不可注入额外参数。
 * 文件头说明：独立 benchmark 源文件，使用模拟 Runtime.exec 表达 sink，不强求编译。
 */
public class ConfusionCmdSanitizedBy {

    static Process exec(String[] cmd) throws Exception {
        // 模拟：Runtime.getRuntime().exec(String[])  —— 数组形式，不经 shell 解析
        return null; // SINK（语义）
    }

    


    static void by(String userInput) throws Exception {
        /*ANCHOR_1*/
        // 仅允许字母数字与少量安全字符，拒绝分号/管道/重定向等注入元字符
        if (!userInput.matches("[a-zA-Z0-9._-]+")) {
            throw new IllegalArgumentException("invalid input");
        }
        // 数组参数化：固定命令 + 单个受控参数，无 shell 解释，无法追加命令
        exec(new String[]{"ls", userInput});
    }
}
