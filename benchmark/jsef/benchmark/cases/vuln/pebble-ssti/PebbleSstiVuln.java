package com.jsef.benchmark.vuln;

import com.mitchellbosecke.pebble.PebbleEngine;

import java.io.StringWriter;
import java.util.Map;

/*
 * JSEF-Benchmark L2 — Pebble SSTI（模板内联用户输入，CWE-1336）
 *
 * 难度：L2（单跳）。用户输入被内联拼接到模板源码，Pebble 把 {{ ... }}
 * 等表达式当作模板语法求值 → 服务端模板注入（SSTI）。
 *
 * CWE-1336 (Improper Neutralization of Special Elements Used in a Template Engine)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用表达式。
 *
 * 修复要点（对照 PebbleSstiSafe.java）：模板源码固定，用户输入仅经 ctx 注入。
 */
public class PebbleSstiVuln {

    public String render(String userInput) throws Exception {
        PebbleEngine engine = new PebbleEngine.Builder().build();
        StringWriter writer = new StringWriter();
        String src = "Hello " + userInput; // 污点：用户输入拼进模板源码
        Map<String, Object> ctx = Map.of("name", userInput);
        // [CHECKPOINT id=JSEF-PEBBLE-001 cwe=1336 level=L2 source=user input in inline template sink=PebbleEngine.getLiteralTemplate(eval).evaluate expect=VULN]
        engine.getLiteralTemplate(src).evaluate(writer, ctx); // [VULN] sink：Pebble 求值模板源码
        return writer.toString();
    }

    public static void main(String[] args) throws Exception {
        new PebbleSstiVuln().render("{{ 7 * 7 }}");
    }
}
