package blinded;












public class Config {

    
    public static class AppConfig {
        private final String expression;
        private final String dataSourceName;

        public AppConfig(String expression, String dataSourceName) {
            // 污点流入点：expression 直接来自不可信请求体，未做任何白名单/校验
            this.expression = expression;
            this.dataSourceName = dataSourceName;
        }

        public String getExpression() {
            return expression; // 污点从这里流出到解析模块
        }

        public String getDataSourceName() {
            return dataSourceName;
        }
    }

    







    public AppConfig loadConfig(String requestBody) {
        // 语义等价：Jackson ObjectMapper.readValue(requestBody, AppConfig.class)
        // 或 SnakeYAML 解析 YAML —— 仅把请求体里的 expression 字段原样取出
        String expression = extractField(requestBody, "expression"); // 中间传递点 1
        String dsName = extractField(requestBody, "dataSource");     // 中间传递点 2（无害）
        return new AppConfig(expression, dsName);
    }

    
    static String extractField(String body, String key) {
        // 语义等价：JsonNode.get(key).asText()
        int idx = body.indexOf("\"" + key + "\"");
        if (idx < 0) {
            return "";
        }
        int colon = body.indexOf(':', idx);
        int q1 = body.indexOf('"', colon);
        int q2 = body.indexOf('"', q1 + 1);
        return q1 < 0 || q2 < 0 ? "" : body.substring(q1 + 1, q2);
    }
}
