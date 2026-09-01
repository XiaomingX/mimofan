package com.jsef.benchmark.sec;

import com.mitchellbosecke.pebble.PebbleEngine;

import java.io.StringWriter;
import java.util.Map;

/*
 * JSEF-Benchmark L2 — Pebble SSTI 修复（CWE-1336）
 *
 * 修复：模板源码固定为常量，用户输入只作为 ctx 变量注入，
 * 永不拼接进模板源码 → 消除模板语法注入面。
 *
 * CWE-1336 (Improper Neutralization of Special Elements Used in a Template Engine)。
 */
public class PebbleSstiSafe {

    private static final String FIXED_TEMPLATE = "Hello {{ name }}";

    public String render(String userInput) throws Exception {
        PebbleEngine engine = new PebbleEngine.Builder().build();
        StringWriter writer = new StringWriter();
        Map<String, Object> ctx = Map.of("name", userInput); // 用户输入仅作 ctx 变量
        // [CHECKPOINT id=JSEF-PEBBLE-001S cwe=1336 level=L2 source=user input (ctx only) sink=PebbleEngine.getLiteralTemplate(fixed).evaluate expect=SAFE]
        engine.getLiteralTemplate(FIXED_TEMPLATE).evaluate(writer, ctx); // 模板源码固定，无注入面
        return writer.toString();
    }

    public static void main(String[] args) throws Exception {
        new PebbleSstiSafe().render("{{ 7 * 7 }}");
    }
}
