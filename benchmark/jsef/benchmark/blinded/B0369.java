/*
 * JSEF Benchmark 真假混淆样本 — 命令注入（B5，CWE-78）
 * Bx 版：exec(userInput) 直接执行用户可控命令。
 * 文件头说明：独立 benchmark 源文件，使用模拟 Runtime.exec 表达 sink，不强求编译。
 */
public class ConfusionCmdSanitized {

    static Process exec(String cmd) throws Exception {
        // 模拟：Runtime.getRuntime().exec
        return null; // SINK（语义）
    }

    


    static void unby(String userInput) throws Exception { // source：不可信用户输入
        /*ANCHOR_1*/
        exec(userInput); // 用户控制完整命令，命令注入
    }
}
