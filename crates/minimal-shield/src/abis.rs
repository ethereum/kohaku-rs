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
        function publishEpochRoot(uint64 epoch) external;
        function claimWithdrawal(address who) external;
        function currentRoot() external view returns (bytes32);
        function currentEpoch() external view returns (uint64);
        function nextIndex() external view returns (uint32);
        function domain(uint64 epoch) external view returns (bytes32);
        function sourceId(uint64 epoch) external view returns (bytes32);
        function withdrawalCredit(address who) external view returns (uint256);

        event LeafAppended(bytes32 indexed cm, uint64 indexed epoch, uint32 index, bytes32 newRoot);
        event NoteSpent(bytes32 indexed nf);
        event EpochRolled(uint64 indexed closedEpoch, bytes32 finalRoot, uint64 indexed newEpoch);
        event RootPublished(uint64 indexed epoch, bytes32 indexed source, bytes32 root);
    }

    #[sol(rpc)]
    contract Groth16Verifier {
        function verifyProof(
            uint256[2] _pA,
            uint256[2][2] _pB,
            uint256[2] _pC,
            uint256[3] _pubSignals
        ) external view returns (bool);
    }

    #[sol(rpc)]
    contract FrameAccount {
        struct Call {
            address target;
            uint256 value;
            bytes data;
        }
        function executeBatch(Call[] calls, bytes signature) external;
        function executeDigest(Call[] calls) external view returns (bytes32);
        function approveSender() external;
        function nonce() external view returns (uint256);
        function owner() external view returns (address);
        function factory() external view returns (address);
    }

    #[sol(rpc)]
    contract FrameAccountFactory {
        function getAddress(address owner, bytes32 salt) external view returns (address);
        function createAccount(address owner, bytes32 salt) external returns (address);
    }

    #[sol(rpc)]
    contract Multicall3 {
        struct Call3 {
            address target;
            bool allowFailure;
            bytes callData;
        }
        struct Result {
            bool success;
            bytes returnData;
        }
        function aggregate3(Call3[] calls) external payable returns (Result[] returnData);
    }

    #[sol(rpc)]
    contract SimpleAccountFactory {
        function getAddress(address owner, uint256 salt) external view returns (address);
        function createAccount(address owner, uint256 salt) external returns (address);
        function accountImplementation() external view returns (address);
    }

    #[sol(rpc)]
    contract SimpleAccount {
        function execute(address dest, uint256 value, bytes func) external;
    }

    #[sol(rpc)]
    contract EntryPoint4337 {
        struct PackedUserOperation {
            address sender;
            uint256 nonce;
            bytes initCode;
            bytes callData;
            bytes32 accountGasLimits;
            uint256 preVerificationGas;
            bytes32 gasFees;
            bytes paymasterAndData;
            bytes signature;
        }
        function handleOps(PackedUserOperation[] ops, address beneficiary) external;
        function getNonce(address sender, uint192 key) external view returns (uint256);
    }
}
