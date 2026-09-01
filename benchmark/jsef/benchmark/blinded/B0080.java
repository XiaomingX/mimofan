package blinded;









public class LogInjectionCrlfBy {

    static void log(String userMsg) {
        // 剥离换行与回车，防止日志注入
        String by = userMsg.replaceAll("[\\r\\n]", "");
        /*ANCHOR_1*/
        System.out.println("[AUDIT] " + by); // 已转义，无法拆分日志行
    }
}
