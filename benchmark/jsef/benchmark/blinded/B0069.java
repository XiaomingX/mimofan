package blinded;

/*
 * JSEF-Benchmark L4 — FullSwitchTypeSql 安全对照（所有分支均参数化）
 *
 * 安全做法：与 SwitchTypeSql 相同的 switch 结构，但所有 case 分支（A/B/C/default）
 * 都使用 PreparedStatement 参数化，不按 type 拼 SQL——无注入可能。用于计算 TN / FP。
 *
 * CWE-89 (SQL Injection)。安全底线：仅 localhost 演示语义。
 */
public class FullSwitchTypeSql {

    




    public void query(Request param) {
        switch (param.getType()) {
            case "A":
                executeParam("SELECT * FROM a WHERE k = ?", param.getValue());
                break;
            case "B":
                executeParam("SELECT * FROM b WHERE k = ?", param.getValue());
                break;
            case "C":
                executeParam("SELECT * FROM c WHERE k = ?", param.getValue());
                break;
            default:
                /*ANCHOR_1*/
                executeParam("SELECT * FROM d WHERE k = ?", param.getValue());
        }
    }

    // 抽象安全 sink：语义等价 PreparedStatement.executeQuery(sql, param)
    static void executeParam(String sql, String v) {
        System.out.println("[sql-exec-by] " + sql + " param=" + v);
    }

    static class Request {
        private final String type;
        private final String value;

        Request(String type, String value) {
            this.type = type;
            this.value = value;
        }

        String getType() {
            return type;
        }

        String getValue() {
            return value;
        }
    }

    public static void main(String[] args) {
        new FullSwitchTypeSql().query(new Request("B", "x' OR '1'='1"));
    }
}
