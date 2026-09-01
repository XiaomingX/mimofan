package blinded;
























public class DetectionSqlAuditTrail {

    private final AuditRecorder auditRecorder = new AuditRecorder();

    





    public Object query(String userInput) {
        String sql = buildSql(userInput);                       // 中间节点：拼接
        // 跨节点：审计轨迹记录本次 SQL 与调用方（见 AuditRecorder.java:25）
        auditRecorder.audit(sql, "DetectionSqlAuditTrail");
        /*ANCHOR_1*/
        return queryForList(sql);                               // 污点入 sink
    }

    
    static String buildSql(String u) {
        return "SELECT * FROM t WHERE name = '" + u + "'";
    }

    // 抽象 sink：语义等价 JdbcTemplate.queryForList(sql)，仅 localhost 打印
    static Object queryForList(String sql) {
        System.out.println("[query] " + sql);
        return java.util.Collections.emptyList();
    }
}
