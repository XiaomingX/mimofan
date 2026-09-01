package blinded;

import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.function.Function;





















public class GroovySandboxEscape {

    
    private static final Set<String> AST_BLACKLIST = new HashSet<>(Arrays.asList("exec", "Runtime"));
    
    private static final Set<String> TYPE_WHITELIST = new HashSet<>(Arrays.asList("String", "Integer", "List"));

    
    static class Scope {
        final java.util.Map<String, Object> vars = new java.util.HashMap<>();
        Object get(String k) { return vars.get(k); }
        void set(String k, Object v) { vars.put(k, v); }
    }

    



    static boolean astCheckPasses(String scriptAstToken) {
        // 编译期只看字面量，漏掉元编程动态构造的调用
        for (String banned : AST_BLACKLIST) {
            if (scriptAstToken.contains(banned)) return false;
        }
        return true; // 看似通过：脚本未直写 "exec"/"Runtime"
    }

    




    public static Object evalViaMetaProgramming(String untrustedScript) {
        Scope scope = new Scope();
        // 沙箱"看似安全"：AST 黑名单拦截了直接危险字面量
        if (!astCheckPasses(untrustedScript)) {
            throw new SecurityException("AST blacklist blocked (static)");
        }

        // 元编程跳板：沙箱内允许的对象被脚本当作起点，运行时反射拿 Runtime
        scope.set("pivot", "localhost-demo");

        // 危险 sink：运行时经元编程/反射逃逸 AST 限制，调用 Runtime.exec
        /*ANCHOR_1*/
        return escapeThroughMeta(scope, "echo localhost-demo");
    }

    



    static Object escapeThroughMeta(Scope scope, String command) {
        try {
            Object pivot = scope.get("pivot");
            Class<?> runtimeClass = Class.forName("java.lang.Runtime");
            java.lang.reflect.Method getRuntime = runtimeClass.getMethod("getRuntime");
            Object runtime = getRuntime.invoke(null);
            java.lang.reflect.Method exec = runtimeClass.getMethod("exec", String.class);
            return exec.invoke(runtime, command);
        } catch (Exception e) {
            throw new RuntimeException("groovy meta-programming escape (demo only)", e);
        }
    }

    public static void main(String[] args) {
        // 仅演示逃逸可达性语义；脚本不含字面量黑名单词，编译期通过、运行时逃逸
        evalViaMetaProgramming("def x = pivot; x");
    }
}
