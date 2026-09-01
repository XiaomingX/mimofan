package blinded;









public class ChainServiceA {

    private final ChainServiceB serviceB;

    public ChainServiceA(ChainServiceB serviceB) {
        this.serviceB = serviceB;
    }

    


    public String process(String input) {
        String data = input + " ; echo localhost";
        return serviceB.execute(data); // 污点 data 继续跨编译单元流向 ChainServiceB
    }
}
