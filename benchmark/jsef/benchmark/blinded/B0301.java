/*
 * JSEF Benchmark 样本 — STR-3 Eval Exclusion-list Bypass 安全对照（CWE-917）
 *
 * 修复策略（与任何具体 Web 框架完全解耦，仅用 Java 标准库）：
 *  - 上下文切换语句（模拟访问权限上下文的特殊引用）在求值前被剥离 / 禁止；
 *    任何试图切换访问权限的子表达式一律拒绝，杜绝「表达式内上下文切换」绕过。
 *  - 排除列表用 Method 对象精确匹配（按方法签名/所属类对象），而非字符串级 token 匹配；
 *    反射字符串拼接变形无法绕过 Method 对象级精确校验。
 *  - L4：被禁方法通过 Method 对象白名单精确允许，任何反射变形调用均被拒绝。
 *
 * 本文件不出现任何具体表达式框架类名或包，纯标准库自包含。
 */

package blinded;

import java.lang.reflect.Method;
import java.util.HashSet;
import java.util.Set;

public class STR3_ExclusionListBypass_By {

    // ------------------------------------------------------------------
    // 安全 EvalEngine：上下文切换语句被剥离 + Method 对象精确匹配
    // ------------------------------------------------------------------
    static class ByEvalEngine {
        // 被禁方法对象级精确列表（按方法对象匹配，非字符串）
        final Set<Method> bannedMethods = new HashSet<>();

        ByEvalEngine() throws NoSuchMethodException {
            // 精确匹配方法对象：Runtime.exec(String)，无法用字符串拼接变形绕过
            bannedMethods.add(Runtime.class.getMethod("exec", String.class));
        }

        // 上下文切换语句（模拟访问权限上下文 / (expr1).(expr2) 串联）在求值前被禁止
        boolean hasContextSwitch(String expr) {
            // 任何切换访问权限的特殊引用一律视为非法
            return expr.contains("getContext") || expr.contains("setAccess")
                    || expr.contains("memberAccess") || expr.contains(".( ");
        }

        // 按 Method 对象精确匹配被禁方法（非字符串级）
        boolean isBannedMethod(Method m) {
            return bannedMethods.contains(m);
        }
    }

    // ------------------------------------------------------------------
    // L3 修复：求值前剥离上下文切换语句，排除检查用精确方法对象匹配
    // ------------------------------------------------------------------

    
    static Object evalBy(String expr) throws Exception {
        ByEvalEngine engine = new ByEvalEngine();

        
        /*ANCHOR_1*/
        if (engine.hasContextSwitch(expr)) {
            return "localhost-demo: rejected (context-switch stripped)";
        }

        
        Method m = Runtime.class.getMethod("exec", String.class);
        if (engine.isBannedMethod(m)) {
            return "localhost-demo: rejected (banned method)";
        }
        return "localhost-demo: by";
    }

    // ------------------------------------------------------------------
    // L4 修复：反射字符串拼接变形 + 上下文切换均被 Method 对象级精确校验拒绝
    // ------------------------------------------------------------------

    
    static Object evalEvasiveBy(String expr) throws Exception {
        ByEvalEngine engine = new ByEvalEngine();

        
        /*ANCHOR_2*/
        if (engine.hasContextSwitch(expr)) {
            return "localhost-demo: rejected (context-switch stripped)";
        }

        
        String bannedName = "ex" + "ec";
        Method m = Runtime.class.getMethod(bannedName, String.class);
        if (engine.isBannedMethod(m)) {
            return "localhost-demo: rejected (banned method by object match)";
        }
        return "localhost-demo: by";
    }
}
