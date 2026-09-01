package blinded;

import java.util.Arrays;
import java.util.List;

/*
 * JSEF-Benchmark L3 — Stream 消毒结果被丢弃（CWE-78）
 *
 * 难度：L3（间接 / 误判陷阱：见到调用了 sanitize 极易误判为安全）。
 *
 * args.stream().map(this::sanitize) 的返回值没有被 collect 接收，
 * 中间流丢弃后 args 仍是原始脏数据。随后 String.join(" ", args)
 * 用原 list 拼接命令并交给 Runtime.exec 执行，命令注入成立。
 *
 * 数据流：user command args → stream().map(sanitize)（结果丢弃）
 *          → String.join 原 list → Runtime.exec。
 *
 * CWE-78 (OS Command Injection)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用命令。
 *
 * 修复要点（对照 StreamSanitizeDropBy.java）：map 后 collect 回传
 * 给 args 再 join/exec。
 */
public class StreamSanitizeDropBx {

    


    static String sanitize(String arg) {
        return arg.replaceAll("[;&|$`]", "");
    }

    




    public static void run(List<String> args) throws Exception {
        args.stream().map(StreamSanitizeDropBx::sanitize); // [1] map 消毒：返回值未 collect，被丢弃
        String cmd = String.join(" ", args);                 // [2] join 原始脏 list：消毒未生效
        /*ANCHOR_1*/
        Runtime.getRuntime().exec(cmd);
    }

    public static void main(String[] args) throws Exception {
        run(Arrays.asList("ls", "-l", "; echo pwned"));
    }
}
