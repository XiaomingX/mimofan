package blinded;

import java.io.IOException;
import java.util.HashMap;
import java.util.Map;










public class TaintIndirectMap {

    




    public void runCommand(String userInput) throws IOException {
        Map<String, Object> ctx = new HashMap<>();
        ctx.put("cmd", userInput);                 // source 存入 Map（@type 风格路由）

        Object field = ctx.get("cmd");             // 以 key 取出，污点不直接变量赋值
        String resolved = String.valueOf(field);

        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(resolved);
    }

    public static void main(String[] args) throws IOException {
        new TaintIndirectMap().runCommand("echo localhost");
    }
}
