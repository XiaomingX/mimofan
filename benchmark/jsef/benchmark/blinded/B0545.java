package blinded;









public class ChainSqlService {

    private final ChainSqlMapper mapper;

    public ChainSqlService(ChainSqlMapper mapper) {
        this.mapper = mapper;
    }

    


    public String process(String input) {
        String sql = "SELECT * FROM items WHERE cat = '" + input + "'";
        return mapper.query(sql); // 污点 sql 继续跨编译单元流向 ChainSqlMapper
    }
}
