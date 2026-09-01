package blinded;











public class LogInjectionCrlf {

    


    static void log(String userMsg) {
        /*ANCHOR_1*/
        System.out.println("[AUDIT] " + userMsg); // 含 \r\n 可拆分伪造日志行
    }
}
