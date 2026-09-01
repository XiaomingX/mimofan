package com.jsef.benchmark.vuln;

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
 * 修复要点（对照 StreamSanitizeDropSafe.java）：map 后 collect 回传
 * 给 args 再 join/exec。
 */
public class StreamSanitizeDropVuln {

    /**
     * sanitize 方法：去除命令元字符（但结果被丢弃，未生效）。
     */
    static String sanitize(String arg) {
        return arg.replaceAll("[;&|$`]", "");
    }

    /**
     * 危险入口：消毒结果未生效，原始脏参数直接拼命令执行。
     *
     * @param args 用户可控命令参数
     */
    public static void run(List<String> args) throws Exception {
        args.stream().map(StreamSanitizeDropVuln::sanitize); // [1] map 消毒：返回值未 collect，被丢弃
        String cmd = String.join(" ", args);                 // [2] join 原始脏 list：消毒未生效
        // [CHECKPOINT id=JSEF-STREAMSAN-001 cwe=78 level=L3 source=user command args sink=Runtime.exec(String.join after discarded map sanitize) expect=VULN trace=benchmark/cases/vuln/stream-sanitize-drop/StreamSanitizeDropVuln.java:39,benchmark/cases/vuln/stream-sanitize-drop/StreamSanitizeDropVuln.java:40,benchmark/cases/vuln/stream-sanitize-drop/StreamSanitizeDropVuln.java:42]
        Runtime.getRuntime().exec(cmd);                      // [3] [VULN] sink：执行含未消毒参数的命令
    }

    public static void main(String[] args) throws Exception {
        run(Arrays.asList("ls", "-l", "; echo pwned"));
    }
}
