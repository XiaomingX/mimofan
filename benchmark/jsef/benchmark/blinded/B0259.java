package blinded;

import java.util.Arrays;
import java.util.HashSet;
import java.util.List;
import java.util.Set;














public class GroovySandboxEscapeBy {

    
    private static final Set<String> AST_BLACKLIST = new HashSet<>(Arrays.asList(
            "exec", "Runtime", "getClass", "forName", "invoke", "getMethod", "reflect"));
    
    private static final Set<String> TYPE_WHITELIST = new HashSet<>(Arrays.asList("String", "Integer", "List"));

    static class Scope {
        final java.util.Map<String, Object> vars = new java.util.HashMap<>();
        Object get(String k) { return vars.get(k); }
    }

    




    public static Object evalViaMetaProgrammingBy(String untrustedScript) {
        Scope scope = new Scope();
        scope.set("pivot", "localhost-demo");

        // 完整 AST 黑名单：任何含反射/危险词的脚本在编译期即被拒绝
        for (String banned : AST_BLACKLIST) {
            if (untrustedScript.contains(banned)) {
                throw new SecurityException("AST blacklist blocked (no meta-programming escape)");
            }
        }

        /*ANCHOR_1*/
        return scope.get("pivot");  // 仅无害字符串，无反射 exec 路径
    }

    public static void main(String[] args) {
        evalViaMetaProgrammingBy("def x = pivot; x");
    }
}
