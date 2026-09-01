package blinded;

















public class AuditLogVault {

    




    public String logAccess(String userId) {

        /*ANCHOR_1*/
        System.out.println("[audit] access by user=" + userId); // 信息泄露：原始 id 入日志
        return userId;
    }
}
