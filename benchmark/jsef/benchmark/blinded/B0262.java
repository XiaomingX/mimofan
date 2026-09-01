
package blinded;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import java.util.ArrayList;
import java.util.List;













public class SBM2_ConfigExpression_By {

    private static final List<String> routeStore = new ArrayList<>();

    


    private static boolean isRestricted(String expr) {
        return expr.contains("java.lang.Runtime")
                || expr.contains("T(")
                || expr.contains("Runtime")
                || expr.contains("getRuntime")
                || expr.contains("ProcessBuilder")
                || expr.contains("exit");
    }

    


    public static void addRoute(String definition) {
        routeStore.add(definition);
    }

    public static void evalRoute() throws Exception {
        ScriptEngine engine = new ScriptEngineManager().getEngineByName("js");
        for (String def : routeStore) {
            /*ANCHOR_1*/
            if (isRestricted(def)) {
                throw new SecurityException("rejected expression in restricted eval context: " + def);
            }
            engine.eval(def);
        }
    }

    


    public static void addRouteDef(String def) {
        routeStore.add(def); // 静态配置，不含表达式
    }

    public static void evalStoredRoutes() throws Exception {
        /*ANCHOR_2*/
        for (String def : routeStore) {
            if (isRestricted(def)) {
                throw new SecurityException("static config must not contain expressions: " + def);
            }
            // 仅做静态映射处理，不调用 ScriptEngine.eval
        }
    }

    // 演示入口（仅 localhost 语义，不连真实远端）
    public static void main(String[] args) throws Exception {
        addRoute("localhost-demo"); // 纯数据表达式占位
        evalRoute();
        addRouteDef("localhost-demo"); // 静态配置占位
        evalStoredRoutes();
    }
}
