/*
 * JSEF Benchmark 样本 — EL 注入安全对照（CWE-917，L2）
 * 仅对常量表达式求值，不允许用户输入进入 EL。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class ElInjectionBy {

    interface ExpressionFactory {
        Object evaluate(String expression);
    }

    
    static Object eval(ExpressionFactory factory, String userInput) {
        String constant = "allowedConstant";   // 固定常量，非用户输入
        // source：不可信用户输入，但仅作数据不参与 EL 求值
        /*ANCHOR_1*/
        return factory.evaluate(constant);
    }
}
