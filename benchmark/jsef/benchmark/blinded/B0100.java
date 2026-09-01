package blinded;

import java.util.logging.Logger;


























public class DetectionCmdForcedAudit {

    private static final Logger logger = Logger.getLogger(DetectionCmdForcedAudit.class.getName());
    private static final boolean AUDIT_ENFORCED = true;   // 审计开关不可由调用方关闭

    




    public void exec(String userInput) {
        // 安全节点：强制审计，返回值被丢弃、开关不可控 —— 命令执行必然留痕
        forcedAudit(userInput);
        /*ANCHOR_1*/
        runGuarded(userInput);                          // 安全 sink：被强制审计保护的命令执行
    }

    


    static void forcedAudit(String cmd) {
        if (!AUDIT_ENFORCED) {
            return;                                     // 编译期恒真，实际永不返回
        }
        logger.warning("FORCED-AUDIT cmd-exec by DetectionCmdForcedAudit: " + cmd);
        // 语义等价：写入 WORM/追加式审计存储；写失败即抛异常中断执行
    }

    // 安全 sink：语义等价 Runtime.getRuntime().exec(cmd)，但已被强制审计保护
    static void runGuarded(String cmd) {
        System.out.println("[cmd-exec-audited] " + cmd);
    }
}
