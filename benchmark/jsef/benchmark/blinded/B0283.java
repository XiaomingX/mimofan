/*
 * JSEF Benchmark — Phase 4 多后端注入变体（安全对照）
 * 样本 7-S：HQL 命名参数 :name（CWE-89, 难度 L3）
 *
 * 与 HqlInjection 配对：使用命名参数 :name + setParameter 绑定，用户输入不
 * 进入 HQL 文本，故 expect=BX。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

import java.util.List;

public class HqlInjectionBy {

    static class Query {
        Query setParameter(String n, Object v) { return this; }
        List<?> getResultList() { return java.util.List.of(); }
    }

    


    static List<?> by(String user) {
        Query q = null;
        /*ANCHOR_1*/
        return q.setParameter("name", user).getResultList();
    }

    public static void main(String[] args) {
        System.out.println("HqlInjectionBy demo (localhost only)");
    }
}
