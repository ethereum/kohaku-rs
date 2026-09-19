use alloy::sol;

sol! {
    #[sol(rpc)]
    contract ShieldedPool {
        function shield(bytes32 inner) external payable returns (uint32);
        struct Spend {
            bytes32 root;
            uint64 rootSlot;
            uint64 epoch;
            bytes32 domain;
            bytes32 nf1;
            bytes32 nf2;
            bytes32 outCm1;
            bytes32 outCm2;
            uint256 publicAmount;
            uint256 fee;
            address recipient;
            address authorizer;
        }
        function settle(Spend s) external;
        function ensureAndClaim(address factory, address owner, bytes32 salt, address who) external;
        function publishEpochRoot(uint64 epoch) external;
        function claimWithdrawal(address who) external;
        function currentRoot() external view returns (bytes32);
        function currentEpoch() external view returns (uint64);
        function nextIndex() external view returns (uint32);
        function domain() external view returns (bytes32);
        function sourceId(uint64 epoch) external view returns (bytes32);
        function withdrawalCredit(address who) external view returns (uint256);

        event LeafAppended(bytes32 indexed cm, uint64 indexed epoch, uint32 index, bytes32 newRoot);
        event NoteSpent(bytes32 indexed nf);
        event EpochRolled(uint64 indexed closedEpoch, bytes32 finalRoot, uint64 indexed newEpoch);
        event RootPublished(uint64 indexed epoch, bytes32 indexed source, bytes32 root);
    }

    #[sol(rpc)]
    contract FrameAccount {
        struct Call {
            address target;
            uint256 value;
            bytes data;
        }
        function executeBatch(Call[] calls) external;
    }

    #[sol(rpc)]
    contract FrameAccountFactory {
        function getAddress(address owner, bytes32 salt) external view returns (address);
        function createAccount(address owner, bytes32 salt) external returns (address);
    }
}
