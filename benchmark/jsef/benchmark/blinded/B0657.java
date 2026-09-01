
package blinded;

import javax.script.ScriptEngine;
import javax.script.ScriptEngineManager;
import java.util.ArrayList;
import java.util.List;

















public class SBM2_ConfigExpression {

    // 模拟配置存储（声明式路由/规则定义库）
    private static final List<String> routeStore = new ArrayList<>();

    



    public static void addRoute(String definition) {
        routeStore.add(definition); // 定义存入配置库（不可信来源）
    }

    public static void evalRoute() throws Exception {
        ScriptEngine engine = new ScriptEngineManager().getEngineByName("js");
        for (String def : routeStore) {
            /*ANCHOR_1*/
            engine.eval(def);
        }
    }

    



    public static void addRouteDef(String def) {
        routeStore.add(def); // 存储阶段：定义持久化（跨阶段起点）
    }

    public static void evalStoredRoutes() throws Exception {
        ScriptEngine engine = new ScriptEngineManager().getEngineByName("js");
        for (String def : routeStore) {
            /*ANCHOR_2*/
            engine.eval(def);
        }
    }

    // 演示入口（仅 localhost 语义，不连真实远端）
    public static void main(String[] args) throws Exception {
        // 不可信 POST 内容（localhost 演示占位）
        addRoute("java.lang.Runtime"); // 占位，非真实利用
        evalRoute();
        addRouteDef("localhost-demo"); // 占位，非真实利用
        evalStoredRoutes();
    }
}
