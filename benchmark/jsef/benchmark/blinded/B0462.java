package blinded;


















public class DataService {

    





    public String fetchSqlFragment(String filter) {
        // 语义等价：restTemplate.getForObject("http://data-svc/sql?f=" + filter, String.class)
        // 出站请求把 filter 传给下游；此处是"下游服务被编排调用的转发方法"。
        String sql = "SELECT * FROM orders WHERE status = '" + filter + "'";
        // 中间节点：下游把拼好的 SQL 片段作为响应原样回传给调用方
        System.out.println("[data-svc] downstream returns fragment");
        return sql;
    }
}
