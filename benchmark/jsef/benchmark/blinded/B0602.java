
package blinded;






public class ReconChainSql_Sink {

    


    public Object runQuery(String sqlFragment) {
        // 语义等价：JdbcTemplate.query("select * from t order by " + sqlFragment)
        System.out.println("[abstract sql] select * from t order by " + sqlFragment);
        return "rows";
    }
}
