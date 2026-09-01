
package blinded;







public class ReconChainSql_Repo {

    private final ReconChainSql_Sink sink;

    public ReconChainSql_Repo(ReconChainSql_Sink sink) {
        this.sink = sink;
    }

    


    public Object findByDynamic(String sortField) {
        String methodName = "findBy" + sortField; // 语义等价：排序字段拼接进方法名
        return sink.runQuery(methodName);
    }
}
