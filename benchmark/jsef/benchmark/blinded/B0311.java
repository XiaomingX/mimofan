package blinded;

import java.util.Optional;
import java.util.stream.Stream;

/*
 * JSEF-Benchmark L4 — StreamPropagation 安全对照（BX 混淆样本）
 *
 * 安全做法：在流入流处理前对输入做白名单校验，命令使用数组形式参数化
 * （无 shell 解析），不可信输入无法注入额外参数。用于计算 TN / FP。
 *
 * CWE-78 (OS Command Injection)。
 */
public class StreamPropagationBy {

    public void run(String userInput) {
        Optional.of(userInput)
                .filter(StreamPropagationBy::isAllowed) // 进入流前校验
                .flatMap(s -> Stream.of(s))
                /*ANCHOR_1*/
                .forEach(this::execAllowed);              // 仅允许值，数组参数化
    }

    // 白名单：仅允许单一安全 token
    static boolean isAllowed(String s) {
        return s.matches("[a-zA-Z0-9_-]{1,32}");
    }

    // 抽象 sink（安全）：语义等价 Runtime.exec(String[])，无 shell 解释
    void execAllowed(String token) {
        System.out.println("[cmd-exec-by] token=" + token);
    }

    public static void main(String[] args) {
        new StreamPropagationBy().run("$(touch /tmp/pwned)");
    }
}
