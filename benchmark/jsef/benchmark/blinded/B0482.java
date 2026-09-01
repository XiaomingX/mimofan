package blinded;

import java.util.logging.Logger;


























public class DetectionCmdAuditLog {

    private static final Logger logger = Logger.getLogger(DetectionCmdAuditLog.class.getName());

    




    public void exec(String userInput) {
        // 监控节点：审计日志把命令写入日志（存在，但可被规避/不构成强制约束）
        logger.warn("cmd-exec: " + userInput);
        /*ANCHOR_1*/
        run(userInput);                            // 污点直连 sink
    }

    // 抽象 sink：语义等价 Runtime.getRuntime().exec(cmd)，仅 localhost 打印
    static void run(String cmd) {
        System.out.println("[cmd-exec] " + cmd);
    }
}
